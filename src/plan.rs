//! `libellus_plan`: change the board in one batch.
//!
//! The whole call runs under one board lock (`locks/board.lock`): read
//! every task, apply every op to a copy in memory, settle the cascades,
//! check the result, and only then write. A bad op means no file changes.
//!
//! One lock rather than a lock per task: an op can touch a task's
//! ancestors (closing and reopening cascade up), its descendants (dropping
//! cascades down) and a whole project (archive), and which tasks those are
//! is only known after reading the board. Per-task locks would have to be
//! taken after that read, in a fixed order, and re-checked; one lock makes
//! the read, the checks and the writes one step, so a parent whose last two
//! children are ticked by two agents at once closes exactly once and no
//! update is lost. Board writes are small and short, so they queue for
//! milliseconds. Reads (`libellus_board`, the brief, the page) take no lock:
//! every file is replaced whole, so a reader sees each task before or after
//! a change, never half of one.

use crate::board::{self, is_open, Board, Task, BY_CHILDREN, MAX_DEPTH};
use crate::fsx;
use crate::json::Value;
use crate::note;
use crate::store::{Locked, Store};
use crate::tool::{field, field_list, field_str, Args};
use std::collections::BTreeSet;

pub const MAX_OPS: usize = 100;
/// Most tasks one outline may create.
pub const MAX_OUTLINE: usize = 200;

/// What a batch changed, for the reply.
#[derive(Default)]
pub struct Report {
    pub added: Vec<String>,
    pub updated: BTreeSet<String>,
    pub removed: Vec<String>,
    pub closed: Vec<String>,
    pub reopened: Vec<String>,
    pub dropped: Vec<String>,
    /// (project id, archive folder name)
    pub archived: Vec<(String, String)>,
}

/// Apply ops to a board in memory. Nothing touches the disk.
pub struct Engine {
    pub board: Board,
    agent: String,
    now: String,
    next_seq: u64,
    first_seq: u64,
    /// Tasks on disk before this call, so writes know create from replace.
    on_disk: BTreeSet<String>,
    open_triggers: Vec<String>,
    close_triggers: Vec<String>,
    pub report: Report,
}

pub struct Add<'a> {
    pub title: &'a str,
    pub id: Option<&'a str>,
    pub parent: Option<&'a str>,
    pub owner: Option<&'a str>,
    pub status: Option<&'a str>,
}

#[derive(Default)]
pub struct Set<'a> {
    pub id: &'a str,
    pub status: Option<&'a str>,
    pub title: Option<&'a str>,
    pub parent: Option<&'a str>,
    pub owner: Option<&'a str>,
    pub outcome: Option<&'a str>,
    pub evidence: Option<Vec<String>>,
}

impl Engine {
    pub fn new(board: Board, agent: &str, now: &str, last_seq: u64) -> Engine {
        let on_disk = board.tasks.keys().cloned().collect();
        Engine {
            board,
            agent: agent.to_string(),
            now: now.to_string(),
            next_seq: last_seq + 1,
            first_seq: last_seq + 1,
            on_disk,
            open_triggers: Vec::new(),
            close_triggers: Vec::new(),
            report: Report::default(),
        }
    }

    pub fn last_seq(&self) -> u64 {
        self.next_seq - 1
    }

    /// True when this call numbered new tasks, so the counter must move.
    pub fn took_seq(&self) -> bool {
        self.next_seq != self.first_seq
    }

    fn touch(&mut self, id: &str) {
        if let Some(t) = self.board.tasks.get_mut(id) {
            t.updated = self.now.clone();
            t.updated_by = self.agent.clone();
        }
        if !self.report.added.iter().any(|a| a == id) {
            self.report.updated.insert(id.to_string());
        }
    }

    fn exists(&self, id: &str) -> Result<&Task, String> {
        self.board.get(id).ok_or_else(|| {
            format!(
                "no task \"{}\" on the board. List ids with libellus_board; archived projects cannot change.",
                fsx::clip(id, 60)
            )
        })
    }

    pub fn add(&mut self, a: &Add) -> Result<String, String> {
        board::validate_title(a.title)?;
        let owner = a.owner.unwrap_or(&self.agent).to_string();
        board::check_agent(&owner, "owner")?;
        let status = a.status.unwrap_or("todo");
        if !board::STATUSES.contains(&status) {
            return Err(format!(
                "status must be one of: {}",
                board::STATUSES.join(", ")
            ));
        }
        let parent = a.parent.unwrap_or("").to_string();
        if !parent.is_empty() {
            self.exists(&parent)?;
            if self.board.depth(&parent) + 1 > MAX_DEPTH {
                return Err(format!(
                    "parent \"{parent}\" is at depth {}; the board is at most {MAX_DEPTH} levels deep",
                    self.board.depth(&parent)
                ));
            }
        }
        let id = match a.id {
            Some(id) => {
                if !board::valid_task_id(id) {
                    return Err(format!(
                        "invalid id \"{}\": one slug of lowercase letters, digits and '-', not a Windows device name",
                        fsx::clip(id, 60)
                    ));
                }
                if self.board.tasks.contains_key(id) {
                    return Err(format!("task \"{id}\" already exists; pick another id"));
                }
                id.to_string()
            }
            None => {
                let mut n = 1;
                loop {
                    let cand = fsx::slug_from(a.title, n, "task");
                    if !self.board.tasks.contains_key(&cand) {
                        break cand;
                    }
                    n += 1;
                }
            }
        };
        let closed = !is_open(status);
        let t = Task {
            id: id.clone(),
            title: a.title.to_string(),
            status: status.to_string(),
            parent: parent.clone(),
            owner,
            created: self.now.clone(),
            updated: self.now.clone(),
            updated_by: self.agent.clone(),
            seq: self.next_seq,
            closed: closed.then(|| self.now.clone()),
            closed_by: closed.then(|| self.agent.clone()),
            outcome: None,
            evidence: Vec::new(),
        };
        self.next_seq += 1;
        self.board.tasks.insert(id.clone(), t);
        self.board.index_added(&id);
        self.report.added.push(id.clone());
        if closed {
            self.close_triggers.push(parent);
        } else {
            self.open_triggers.push(id.clone());
        }
        Ok(id)
    }

    pub fn set(&mut self, s: &Set) -> Result<(), String> {
        let old = self.exists(s.id)?.clone();
        let id = s.id.to_string();
        if s.status.is_none()
            && s.title.is_none()
            && s.parent.is_none()
            && s.owner.is_none()
            && s.outcome.is_none()
            && s.evidence.is_none()
        {
            return Err(
                "set needs at least one of status, title, parent, owner, outcome, evidence".into(),
            );
        }
        if let Some(title) = s.title {
            board::validate_title(title)?;
        }
        if let Some(owner) = s.owner {
            board::check_agent(owner, "owner")?;
        }
        if let Some(o) = s.outcome {
            if !o.is_empty() {
                note::validate_outcome(o)?;
            }
        }
        if let Some(ev) = &s.evidence {
            if let Some(bad) = ev.iter().find(|p| !note::valid_pointer(p)) {
                return Err(format!(
                    "bad evidence pointer \"{}\": use tool:reference with no spaces, commas or brackets",
                    fsx::clip(bad, 60)
                ));
            }
        }
        if let Some(status) = s.status {
            if !board::STATUSES.contains(&status) {
                return Err(format!(
                    "status must be one of: {}",
                    board::STATUSES.join(", ")
                ));
            }
        }
        if let Some(np) = s.parent {
            if np != old.parent {
                self.move_task(&id, np)?;
            }
        }
        {
            let t = self.board.tasks.get_mut(&id).expect("checked above");
            if let Some(title) = s.title {
                t.title = title.to_string();
            }
            if let Some(owner) = s.owner {
                t.owner = owner.to_string();
            }
            if let Some(o) = s.outcome {
                t.outcome = (!o.is_empty()).then(|| o.to_string());
            }
            if let Some(ev) = &s.evidence {
                let mut seen: Vec<String> = Vec::new();
                for p in ev {
                    if !seen.contains(p) {
                        seen.push(p.clone());
                    }
                }
                t.evidence = seen;
            }
        }
        if let Some(status) = s.status {
            self.change_status(&id, status, &self.agent.clone());
        }
        self.touch(&id);
        Ok(())
    }

    fn move_task(&mut self, id: &str, new_parent: &str) -> Result<(), String> {
        if !new_parent.is_empty() {
            self.exists(new_parent)?;
            if new_parent == id || self.board.path(new_parent).iter().any(|p| p == id) {
                return Err(format!(
                    "cannot move \"{id}\" under \"{new_parent}\": that task is inside it"
                ));
            }
            let deepest = self.board.depth(new_parent) + self.board.height(id);
            if deepest > MAX_DEPTH {
                return Err(format!(
                    "moving \"{id}\" under \"{new_parent}\" would make the tree {deepest} levels deep; at most {MAX_DEPTH}"
                ));
            }
        }
        let old_parent = self.board.tasks[id].parent.clone();
        self.board.tasks.get_mut(id).expect("checked").parent = new_parent.to_string();
        self.board.reindex();
        self.close_triggers.push(old_parent);
        if is_open(&self.board.tasks[id].status) {
            self.open_triggers.push(id.to_string());
        } else {
            self.close_triggers.push(new_parent.to_string());
        }
        Ok(())
    }

    fn change_status(&mut self, id: &str, status: &str, by: &str) {
        let t = self.board.tasks.get_mut(id).expect("checked");
        let was_open = is_open(&t.status);
        if t.status == status {
            return;
        }
        t.status = status.to_string();
        let parent = t.parent.clone();
        if is_open(status) {
            t.closed = None;
            t.closed_by = None;
            self.open_triggers.push(id.to_string());
            return;
        }
        if was_open {
            t.closed = Some(self.now.clone());
        }
        t.closed_by = Some(by.to_string());
        if status == "dropped" {
            for d in self.board.descendants(id) {
                let dt = self.board.tasks.get_mut(&d).expect("indexed");
                if is_open(&dt.status) {
                    dt.status = "dropped".to_string();
                    dt.closed = Some(self.now.clone());
                    dt.closed_by = Some(by.to_string());
                    self.report.dropped.push(d.clone());
                    self.touch(&d);
                }
            }
        }
        self.close_triggers.push(parent);
    }

    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let t = self.exists(id)?;
        if t.status != "todo" || !self.board.children(id).is_empty() {
            return Err(format!(
                "remove is only for a todo task with no children (a mistake). \"{id}\" is {}{}; set its status to dropped instead",
                t.status,
                if self.board.children(id).is_empty() { "" } else { " and has children" }
            ));
        }
        let parent = t.parent.clone();
        self.board.tasks.remove(id);
        self.board.reindex();
        self.report.added.retain(|a| a != id);
        self.report.updated.remove(id);
        if self.on_disk.contains(id) {
            self.report.removed.push(id.to_string());
        }
        self.close_triggers.push(parent);
        Ok(())
    }

    /// Settle the cascades and check the result. After this, the board in
    /// memory is what will be written.
    pub fn finish(&mut self) -> Result<(), String> {
        // Reopen: an open task under a parent closed by its children reopens
        // that parent, and so on up. A parent an agent closed is an error.
        let triggers = std::mem::take(&mut self.open_triggers);
        for id in triggers {
            let Some(t) = self.board.get(&id) else {
                continue;
            };
            if !is_open(&t.status) {
                continue;
            }
            let mut cur = t.parent.clone();
            while let Some(p) = self.board.get(&cur).cloned() {
                if is_open(&p.status) {
                    break;
                }
                if p.closed_by.as_deref() != Some(BY_CHILDREN) {
                    return Err(format!(
                        "task \"{id}\" is open under \"{}\", which is {} (closed by {}). Set \"{}\" to todo or doing first, in the same call if you like.",
                        p.id,
                        p.status,
                        p.closed_by.as_deref().unwrap_or("?"),
                        p.id
                    ));
                }
                let pt = self.board.tasks.get_mut(&cur).expect("exists");
                pt.status = "doing".to_string();
                pt.closed = None;
                pt.closed_by = None;
                self.report.reopened.push(cur.clone());
                self.touch(&cur);
                cur = p.parent.clone();
            }
        }
        // A closed task never has an open child.
        let mut touched: Vec<String> = self.report.updated.iter().cloned().collect();
        touched.extend(self.report.added.iter().cloned());
        for id in &touched {
            let Some(t) = self.board.get(id) else {
                continue;
            };
            if is_open(&t.status) {
                continue;
            }
            let open: Vec<&String> = self
                .board
                .children(id)
                .iter()
                .filter(|c| self.board.get(c).is_some_and(|c| is_open(&c.status)))
                .collect();
            if !open.is_empty() {
                let names: Vec<&str> = open.iter().take(10).map(|s| s.as_str()).collect();
                let more = if open.len() > 10 {
                    format!(" and {} more", open.len() - 10)
                } else {
                    String::new()
                };
                return Err(format!(
                    "cannot mark \"{id}\" {} while it has open children: {}{more}. Close or drop them first (the same call is fine).",
                    t.status,
                    names.join(", ")
                ));
            }
        }
        // Close: a parent whose children are all done or dropped, with at
        // least one done, becomes done, and so on up to the project.
        let triggers = std::mem::take(&mut self.close_triggers);
        let mut seen = BTreeSet::new();
        for start in triggers {
            let mut cur = start;
            while !cur.is_empty() && seen.insert(cur.clone()) {
                let Some(p) = self.board.get(&cur) else {
                    break;
                };
                if !is_open(&p.status) {
                    break;
                }
                let kids = self.board.children(&cur);
                let all_closed = !kids.is_empty()
                    && kids
                        .iter()
                        .all(|k| self.board.get(k).is_some_and(|k| !is_open(&k.status)));
                let any_done = kids
                    .iter()
                    .any(|k| self.board.get(k).is_some_and(|k| k.status == "done"));
                if !(all_closed && any_done) {
                    break;
                }
                let next = p.parent.clone();
                let pt = self.board.tasks.get_mut(&cur).expect("exists");
                pt.status = "done".to_string();
                pt.closed = Some(self.now.clone());
                pt.closed_by = Some(BY_CHILDREN.to_string());
                self.report.closed.push(cur.clone());
                self.touch(&cur);
                // `seen` stops a second walk, but a parent must be checked
                // again once its child closed here.
                seen.remove(&next);
                cur = next;
            }
        }
        Ok(())
    }

    /// Closed projects, to move into the archive after the writes.
    pub fn closed_projects(&self) -> Vec<String> {
        self.board
            .roots()
            .iter()
            .filter(|r| {
                self.board
                    .get(r)
                    .is_some_and(|t| t.parent.is_empty() && !is_open(&t.status))
            })
            .cloned()
            .collect()
    }

    pub fn is_new(&self, id: &str) -> bool {
        !self.on_disk.contains(id)
    }
}

/// One outline line, parsed.
pub struct OutlineItem {
    pub line: usize,
    pub level: usize,
    pub status: &'static str,
    pub title: String,
}

/// Parse a markdown checkbox list. Indentation is two spaces or one tab per
/// level. `[ ]` todo, `[~]` doing, `[!]` blocked, `[x]` done, `[-]` dropped.
/// Blank lines are skipped. Anything else is an error naming the line.
pub fn parse_outline(text: &str) -> Result<Vec<OutlineItem>, String> {
    let mut out: Vec<OutlineItem> = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line_no = i + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            continue;
        }
        let bad = |why: &str| format!("outline line {line_no}: {why}");
        let indent: &str = &line[..line.len() - line.trim_start().len()];
        let level = if indent.is_empty() {
            0
        } else if indent.bytes().all(|b| b == b'\t') {
            indent.len()
        } else if indent.bytes().all(|b| b == b' ') {
            if indent.len() % 2 != 0 {
                return Err(bad("indent with two spaces (or one tab) per level"));
            }
            indent.len() / 2
        } else {
            return Err(bad("indent with spaces or tabs, not both"));
        };
        let rest = line.trim_start();
        let Some(rest) = rest.strip_prefix("- [") else {
            return Err(bad("each line is \"- [ ] Title\""));
        };
        let mut chars = rest.chars();
        let status = match chars.next() {
            Some(' ') => "todo",
            Some('~') => "doing",
            Some('!') => "blocked",
            Some('x') => "done",
            Some('-') => "dropped",
            _ => {
                return Err(bad(
                    "the box is [ ] todo, [~] doing, [!] blocked, [x] done or [-] dropped",
                ))
            }
        };
        let after = chars.as_str();
        let Some(title) = after.strip_prefix("] ") else {
            return Err(bad(
                "close the box and put a space before the title: \"- [ ] Title\"",
            ));
        };
        let title = title.trim();
        board::validate_title(title).map_err(|e| bad(&e))?;
        let prev = out.last().map(|p| p.level);
        match prev {
            None if level != 0 => return Err(bad("the first line is not indented")),
            Some(p) if level > p + 1 => {
                return Err(bad("indented more than one level below the line above"))
            }
            _ => {}
        }
        if level + 1 > MAX_DEPTH {
            return Err(bad(&format!("deeper than {MAX_DEPTH} levels")));
        }
        out.push(OutlineItem {
            line: line_no,
            level,
            status,
            title: title.to_string(),
        });
    }
    if out.is_empty() {
        return Err("outline has no tasks".into());
    }
    if out.len() > MAX_OUTLINE {
        return Err(format!(
            "outline has {} tasks; at most {MAX_OUTLINE} in one call",
            out.len()
        ));
    }
    Ok(out)
}

/// Add an outline under `parent` (or as new projects).
pub fn apply_outline(
    e: &mut Engine,
    items: &[OutlineItem],
    parent: Option<&str>,
) -> Result<(), String> {
    let mut stack: Vec<String> = Vec::new();
    for item in items {
        stack.truncate(item.level);
        let under = match stack.last() {
            Some(p) => Some(p.as_str()),
            None => parent,
        };
        let id = e
            .add(&Add {
                title: &item.title,
                id: None,
                parent: under,
                owner: None,
                status: Some(item.status),
            })
            .map_err(|err| format!("outline line {}: {err}", item.line))?;
        stack.push(id);
    }
    Ok(())
}

fn apply_op(e: &mut Engine, op: &Value) -> Result<(), String> {
    let Value::Object(pairs) = op else {
        return Err("an op must be an object".into());
    };
    let keys: Vec<&str> = pairs
        .iter()
        .filter(|(_, v)| !matches!(v, Value::Null))
        .map(|(k, _)| k.as_str())
        .collect();
    if keys.len() != 1 {
        return Err("each op has exactly one of add, set or remove".into());
    }
    match keys[0] {
        "add" => {
            let v = field(op, "add").expect("present");
            e.add(&Add {
                title: field_str(v, "title").unwrap_or_default(),
                id: field_str(v, "id"),
                parent: field_str(v, "parent").filter(|p| !p.is_empty()),
                owner: field_str(v, "owner"),
                status: field_str(v, "status"),
            })
            .map(|_| ())
        }
        "set" => {
            let v = field(op, "set").expect("present");
            e.set(&Set {
                id: field_str(v, "id").unwrap_or_default(),
                status: field_str(v, "status"),
                title: field_str(v, "title"),
                parent: field_str(v, "parent"),
                owner: field_str(v, "owner"),
                outcome: field_str(v, "outcome"),
                evidence: field_list(v, "evidence"),
            })
        }
        _ => e.remove(field_str(op, "remove").unwrap_or_default()),
    }
}

fn op_label(op: &Value) -> String {
    if let Some(v) = field(op, "add") {
        return format!(
            "add \"{}\"",
            fsx::clip(field_str(v, "title").unwrap_or(""), 40)
        );
    }
    if let Some(v) = field(op, "set") {
        return format!("set \"{}\"", field_str(v, "id").unwrap_or(""));
    }
    format!("remove \"{}\"", field_str(op, "remove").unwrap_or(""))
}

pub fn run(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    board::check_agent(agent, "agent")?;
    let ops = a.objects("ops").unwrap_or(&[]);
    let outline = a.str("outline");
    if a.has("parent") && outline.is_none() {
        return Err(
            "parent places an outline; pass outline with it, or use ops add with parent".into(),
        );
    }
    if ops.is_empty() && outline.is_none() {
        return Err("pass ops (add, set, remove) or an outline".into());
    }
    // Parse the outline before taking the lock: a typo costs nothing.
    let items = match outline {
        Some(text) => Some(parse_outline(text)?),
        None => None,
    };
    let dir = board::board_dir(&store)?;
    fsx::make_private_dir(&dir)?;
    let _lock = store.lock(Locked::Board)?;
    let current = board::load(&store)?;
    let counter = dir.join("seq");
    let last_seq = match crate::seq::read_counter(&counter)? {
        Some(v) => v.max(current.max_seq()),
        None => current.max_seq(),
    };
    let now = fsx::now_iso();
    let mut e = Engine::new(current, agent, &now, last_seq);
    for (i, op) in ops.iter().enumerate() {
        apply_op(&mut e, op)
            .map_err(|err| format!("nothing written: ops[{i}] ({}): {err}", op_label(op)))?;
    }
    if let Some(items) = &items {
        apply_outline(&mut e, items, a.str("parent").filter(|p| !p.is_empty()))
            .map_err(|err| format!("nothing written: {err}"))?;
    }
    e.finish()
        .map_err(|err| format!("nothing written: {err}"))?;
    let archive = e.closed_projects();
    let written = save(&store, &mut e, &counter, &archive)?;
    let html = crate::html::write_page(&store, &e.board);
    Ok(reply(&e.report, written, html))
}

/// Write the changes found by the engine. Called under the board lock.
fn save(
    store: &Store,
    e: &mut Engine,
    counter: &std::path::Path,
    archive: &[String],
) -> Result<usize, String> {
    let dir = board::board_dir(store)?;
    if e.took_seq() {
        crate::seq::write_counter(counter, e.last_seq())?;
    }
    let mut ids: Vec<String> = e.report.added.clone();
    ids.extend(e.report.updated.iter().cloned());
    let mut done: Vec<String> = Vec::new();
    let stop = |done: &[String], what: String| {
        format!(
            "board write stopped: {what}. Already written: {}. Run libellus check, then repeat the rest.",
            if done.is_empty() { "none".to_string() } else { done.join(", ") }
        )
    };
    for id in &ids {
        let Some(t) = e.board.get(id) else {
            continue;
        };
        let path = board::task_path(&dir, id)?;
        fsx::refuse_special(&path)?;
        fsx::write_atomic(&path, t.render().as_bytes(), !e.is_new(id))
            .map_err(|err| stop(&done, format!("task \"{id}\": {err}")))?;
        done.push(id.clone());
    }
    for id in &e.report.removed {
        let path = board::task_path(&dir, id)?;
        std::fs::remove_file(&path)
            .map_err(|err| stop(&done, format!("cannot remove {}: {err}", path.display())))?;
        done.push(format!("removed {id}"));
    }
    for project in archive {
        let base = board::archive_dir(store)?;
        fsx::make_private_dir(&base)?;
        let mut n = 1;
        let (name, target) = loop {
            let name = if n == 1 {
                project.clone()
            } else {
                format!("{project}-{n}")
            };
            let target = fsx::join_id(&base, &name, 1)?;
            if !fsx::path_exists_nofollow(&target) {
                break (name, target);
            }
            n += 1;
        };
        fsx::make_private_dir(&target)?;
        let mut subtree = vec![project.clone()];
        subtree.extend(e.board.descendants(project));
        for id in &subtree {
            let from = board::task_path(&dir, id)?;
            let to = board::task_path(&target, id)?;
            fsx::rename(&from, &to).map_err(|err| {
                stop(
                    &done,
                    format!("cannot move {} to {}: {err}", from.display(), to.display()),
                )
            })?;
        }
        for id in &subtree {
            e.board.tasks.remove(id);
        }
        e.board.reindex();
        e.report.archived.push((project.clone(), name));
    }
    Ok(done.len())
}

fn reply(r: &Report, written: usize, html: Result<std::path::PathBuf, String>) -> Value {
    let list = |v: &[String]| Value::Array(v.iter().map(Value::str).collect());
    let mut p: Vec<(&str, Value)> = Vec::new();
    if !r.added.is_empty() {
        p.push(("added", list(&r.added)));
    }
    let updated: Vec<String> = r
        .updated
        .iter()
        .filter(|u| !r.closed.contains(u) && !r.reopened.contains(u) && !r.dropped.contains(u))
        .cloned()
        .collect();
    if !updated.is_empty() {
        p.push(("updated", list(&updated)));
    }
    if !r.removed.is_empty() {
        p.push(("removed", list(&r.removed)));
    }
    if !r.closed.is_empty() {
        p.push(("closed_by_children", list(&r.closed)));
    }
    if !r.reopened.is_empty() {
        p.push(("reopened", list(&r.reopened)));
    }
    if !r.dropped.is_empty() {
        p.push(("dropped", list(&r.dropped)));
    }
    if !r.archived.is_empty() {
        p.push((
            "archived",
            Value::Array(
                r.archived
                    .iter()
                    .map(|(id, name)| {
                        Value::obj(vec![
                            ("project", Value::str(id)),
                            ("folder", Value::str(name)),
                        ])
                    })
                    .collect(),
            ),
        ));
    }
    p.push(("files_written", Value::uint(written as u64)));
    match html {
        Ok(path) => p.push(("html", Value::String(path.display().to_string()))),
        Err(e) => p.push(("html_error", Value::String(e))),
    }
    Value::obj(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_levels_and_boxes() {
        let items = parse_outline("- [ ] P\n  - [~] A\n\t\t- [x] B\n  - [!] C\n- [-] Q\n").unwrap();
        let got: Vec<(usize, &str)> = items.iter().map(|i| (i.level, i.status)).collect();
        assert_eq!(
            got,
            vec![
                (0, "todo"),
                (1, "doing"),
                (2, "done"),
                (1, "blocked"),
                (0, "dropped")
            ]
        );
    }
}
