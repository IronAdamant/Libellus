//! The board: one shared tree of tasks for every agent in the store.
//!
//! ```text
//! .libellus/board/<id>.md                   one task per file
//! .libellus/board/seq                       the board counter
//! .libellus/board/archive/<project>/<id>.md closed projects, kept for history
//! .libellus/board.html                      the read-only page for people
//! ```
//!
//! Notes stay private per agent. The board is the deliberate shared
//! exception: a main agent plans a project and hands subtrees to sub-agents,
//! and every agent ticks off its own tasks. Reading is O(tasks): every file
//! is read once, and the tree is built with one index.

use crate::fsx;
use crate::json::Value;
use crate::note;
use crate::store::Store;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const STATUSES: &[&str] = &["todo", "doing", "blocked", "done", "dropped"];
/// The kanban columns, in order. Dropped tasks are shown apart, collapsed.
pub const COLUMNS: &[&str] = &["todo", "doing", "blocked", "done"];
/// A project is depth 1. Its deepest task is at most depth 6.
pub const MAX_DEPTH: usize = 6;
/// Largest task file read. A task is a few hundred bytes.
pub const MAX_TASK_BYTES: u64 = 65_536;
/// `closed_by` for a parent closed because all its children closed.
pub const BY_CHILDREN: &str = "children";

pub fn is_open(status: &str) -> bool {
    matches!(status, "todo" | "doing" | "blocked")
}

/// The checkbox for a status, as in an outline: `[ ]`, `[~]`, `[!]`, `[x]`, `[-]`.
pub fn mark(status: &str) -> &'static str {
    match status {
        "doing" => "[~]",
        "blocked" => "[!]",
        "done" => "[x]",
        "dropped" => "[-]",
        _ => "[ ]",
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: String,
    pub title: String,
    pub status: String,
    /// Empty for a top-level project.
    pub parent: String,
    pub owner: String,
    pub created: String,
    pub updated: String,
    pub updated_by: String,
    /// Creation order across the board. Siblings are listed by it.
    pub seq: u64,
    pub closed: Option<String>,
    /// An agent, or `children`.
    pub closed_by: Option<String>,
    pub outcome: Option<String>,
    pub evidence: Vec<String>,
}

/// An agent name the board accepts as an owner or a writer.
pub fn valid_agent(name: &str) -> bool {
    fsx::valid_slug(name) && !fsx::reserved_name(name) && name != BY_CHILDREN
}

pub fn check_agent(name: &str, what: &str) -> Result<(), String> {
    if name == BY_CHILDREN {
        return Err(format!(
            "{what} \"children\" is reserved: it marks a parent closed by its children"
        ));
    }
    if !valid_agent(name) {
        return Err(format!(
            "invalid {what} \"{}\": use lowercase letters, digits and '-' (main, sub-2), not a Windows device name",
            fsx::clip(name, 40)
        ));
    }
    Ok(())
}

/// A task id: one slug, not a Windows device name.
pub fn valid_task_id(id: &str) -> bool {
    fsx::valid_slug(id) && !fsx::reserved_name(id)
}

const KEYS: &[&str] = &[
    "title",
    "status",
    "parent",
    "owner",
    "created",
    "updated",
    "updated_by",
    "seq",
    "closed",
    "closed_by",
    "outcome",
    "evidence",
];

impl Task {
    pub fn render(&self) -> String {
        let mut s = String::from("---\n");
        s.push_str(&format!("title: {}\n", self.title));
        s.push_str(&format!("status: {}\n", self.status));
        if self.parent.is_empty() {
            s.push_str("parent:\n");
        } else {
            s.push_str(&format!("parent: {}\n", self.parent));
        }
        s.push_str(&format!(
            "owner: {}\ncreated: {}\nupdated: {}\nupdated_by: {}\nseq: {}\n",
            self.owner, self.created, self.updated, self.updated_by, self.seq
        ));
        if let Some(c) = &self.closed {
            s.push_str(&format!("closed: {c}\n"));
        }
        if let Some(c) = &self.closed_by {
            s.push_str(&format!("closed_by: {c}\n"));
        }
        if let Some(o) = &self.outcome {
            s.push_str(&format!("outcome: {o}\n"));
        }
        if !self.evidence.is_empty() {
            s.push_str(&format!("evidence: [{}]\n", self.evidence.join(", ")));
        }
        s.push_str("---\n");
        s
    }

    /// Strict, like a note: an unknown or repeated key, a bad value, a
    /// missing field, or text after the frontmatter is an error.
    pub fn parse(id: &str, text: &str) -> Result<Task, String> {
        let unix;
        let text = if text.contains("\r\n") {
            unix = text.replace("\r\n", "\n");
            unix.as_str()
        } else {
            text
        };
        let mut lines = text.split('\n');
        if lines.next() != Some("---") {
            return Err("does not start with a --- frontmatter line".into());
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line == "---" {
                closed = true;
                break;
            }
            if line.trim().is_empty() {
                continue;
            }
            let (k, v) = line
                .split_once(':')
                .ok_or_else(|| format!("bad frontmatter line \"{}\"", fsx::clip(line, 60)))?;
            let (k, v) = (k.trim(), v.trim());
            if !KEYS.contains(&k) {
                return Err(format!("unknown frontmatter key \"{k}\""));
            }
            if fields.iter().any(|(fk, _)| fk == k) {
                return Err(format!("duplicate frontmatter key \"{k}\""));
            }
            fields.push((k.to_string(), v.to_string()));
        }
        if !closed {
            return Err("frontmatter is never closed with ---".into());
        }
        if lines.any(|l| !l.trim().is_empty()) {
            return Err("text after the frontmatter; a task has no body".into());
        }
        let get = |k: &str| {
            fields
                .iter()
                .find(|(fk, _)| fk == k)
                .map(|(_, v)| v.clone())
        };
        let need = |k: &str| {
            get(k)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("missing frontmatter field \"{k}\""))
        };
        let title = need("title")?;
        validate_title(&title)?;
        let status = need("status")?;
        if !STATUSES.contains(&status.as_str()) {
            return Err(format!(
                "status \"{status}\" is not one of: {}",
                STATUSES.join(", ")
            ));
        }
        let parent = get("parent").ok_or("missing frontmatter field \"parent\"")?;
        if !parent.is_empty() && !valid_task_id(&parent) {
            return Err(format!(
                "parent \"{}\" is not a task id",
                fsx::clip(&parent, 60)
            ));
        }
        if parent == id {
            return Err("a task cannot be its own parent".into());
        }
        let owner = need("owner")?;
        check_agent(&owner, "owner")?;
        let updated_by = need("updated_by")?;
        check_agent(&updated_by, "updated_by")?;
        let stamp = |k: &str, v: &str| -> Result<(), String> {
            if fsx::valid_timestamp(v) {
                Ok(())
            } else {
                Err(format!("{k} \"{v}\" is not YYYY-MM-DDTHH:MM:SSZ"))
            }
        };
        let created = need("created")?;
        stamp("created", &created)?;
        let updated = need("updated")?;
        stamp("updated", &updated)?;
        let seq = note::parse_seq(&need("seq")?).map_err(|e| format!("seq {e}"))?;
        let closed_at = get("closed").filter(|v| !v.is_empty());
        if let Some(c) = &closed_at {
            stamp("closed", c)?;
        }
        let closed_by = get("closed_by").filter(|v| !v.is_empty());
        if let Some(by) = &closed_by {
            if by != BY_CHILDREN {
                check_agent(by, "closed_by")?;
            }
        }
        if is_open(&status) && (closed_at.is_some() || closed_by.is_some()) {
            return Err(format!("status {status} is open but closed is set"));
        }
        if !is_open(&status) && (closed_at.is_none() || closed_by.is_none()) {
            return Err(format!("status {status} needs closed and closed_by"));
        }
        let outcome = get("outcome").filter(|v| !v.is_empty());
        if let Some(o) = &outcome {
            note::validate_outcome(o)?;
        }
        let evidence = match get("evidence") {
            None => Vec::new(),
            Some(v) => v
                .strip_prefix('[')
                .and_then(|r| r.strip_suffix(']'))
                .ok_or("evidence must be written as [a, b]")?
                .split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect(),
        };
        if let Some(bad) = evidence.iter().find(|p| !note::valid_pointer(p)) {
            return Err(format!(
                "bad evidence pointer \"{bad}\" (use tool:reference)"
            ));
        }
        Ok(Task {
            id: id.to_string(),
            title,
            status,
            parent,
            owner,
            created,
            updated,
            updated_by,
            seq,
            closed: closed_at,
            closed_by,
            outcome,
            evidence,
        })
    }
}

pub fn validate_title(s: &str) -> Result<(), String> {
    note::validate_summary(s).map_err(|e| e.replacen("summary", "title", 1))
}

/// The live board in memory, with a child index. Orphans (a parent that is
/// not on the board) are indexed as roots and named in `warnings`.
#[derive(Clone, Default)]
pub struct Board {
    pub tasks: HashMap<String, Task>,
    kids: HashMap<String, Vec<String>>,
    pub warnings: Vec<String>,
}

impl Board {
    pub fn from_tasks(list: Vec<Task>) -> Result<Board, String> {
        let mut b = Board::default();
        for t in list {
            b.tasks.insert(t.id.clone(), t);
        }
        b.reindex();
        // A loop or a chain deeper than the limit can only come from a hand
        // edit. It is damage, not something to guess around.
        for id in b.tasks.keys() {
            let mut cur = id.as_str();
            let mut depth = 1;
            while let Some(t) = b.tasks.get(cur) {
                if t.parent.is_empty() || !b.tasks.contains_key(&t.parent) {
                    break;
                }
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(format!(
                        "board task \"{id}\" is more than {MAX_DEPTH} levels deep, or its parents form a loop. Fix the parent fields by hand."
                    ));
                }
                cur = &t.parent;
            }
        }
        let mut orphans: Vec<&Task> = b
            .tasks
            .values()
            .filter(|t| !t.parent.is_empty() && !b.tasks.contains_key(&t.parent))
            .collect();
        orphans.sort_by(|x, y| x.id.cmp(&y.id));
        let notes: Vec<String> = orphans
            .iter()
            .map(|t| {
                format!(
                    "board task \"{}\" names parent \"{}\", which is not on the board. Set its parent with libellus_plan, or move the file into board/archive/ if its project was archived.",
                    t.id, t.parent
                )
            })
            .collect();
        b.warnings.extend(notes);
        Ok(b)
    }

    /// Rebuild the child index, siblings in creation order.
    pub fn reindex(&mut self) {
        let mut kids: HashMap<String, Vec<String>> = HashMap::new();
        for t in self.tasks.values() {
            let key = if t.parent.is_empty() || !self.tasks.contains_key(&t.parent) {
                String::new()
            } else {
                t.parent.clone()
            };
            kids.entry(key).or_default().push(t.id.clone());
        }
        for list in kids.values_mut() {
            list.sort_by(|a, b| {
                let (x, y) = (&self.tasks[a], &self.tasks[b]);
                x.seq.cmp(&y.seq).then_with(|| x.id.cmp(&y.id))
            });
        }
        self.kids = kids;
    }

    /// Index a task just inserted. Its `seq` is the newest, so it goes last
    /// among its siblings.
    pub fn index_added(&mut self, id: &str) {
        let parent = self.tasks[id].parent.clone();
        let key = if self.tasks.contains_key(&parent) {
            parent
        } else {
            String::new()
        };
        self.kids.entry(key).or_default().push(id.to_string());
    }

    pub fn children(&self, id: &str) -> &[String] {
        self.kids.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn roots(&self) -> &[String] {
        self.children("")
    }

    pub fn get(&self, id: &str) -> Option<&Task> {
        self.tasks.get(id)
    }

    /// 1 for a project.
    pub fn depth(&self, id: &str) -> usize {
        self.path(id).len()
    }

    /// Ids from the project down to `id`.
    pub fn path(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = id;
        while let Some(t) = self.tasks.get(cur) {
            out.push(t.id.clone());
            if out.len() > MAX_DEPTH + 1 || t.parent.is_empty() {
                break;
            }
            cur = &t.parent;
        }
        out.reverse();
        out
    }

    /// The project (root) above `id`.
    pub fn project_of(&self, id: &str) -> String {
        self.path(id).into_iter().next().unwrap_or_default()
    }

    /// Levels in the subtree under and including `id`. A leaf is 1.
    pub fn height(&self, id: &str) -> usize {
        1 + self
            .children(id)
            .iter()
            .map(|c| self.height(c))
            .max()
            .unwrap_or(0)
    }

    /// Every task under `id`, not `id` itself, parents before children.
    pub fn descendants(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack: Vec<&String> = self.children(id).iter().rev().collect();
        while let Some(c) = stack.pop() {
            out.push(c.clone());
            stack.extend(self.children(c).iter().rev());
        }
        out
    }

    /// Every task depth first, siblings in creation order, with its depth.
    pub fn tree_order(&self) -> Vec<(&Task, usize)> {
        let mut out = Vec::with_capacity(self.tasks.len());
        let mut stack: Vec<(&String, usize)> = self.roots().iter().rev().map(|r| (r, 1)).collect();
        while let Some((id, depth)) = stack.pop() {
            let Some(t) = self.tasks.get(id) else {
                continue;
            };
            out.push((t, depth));
            stack.extend(self.children(id).iter().rev().map(|c| (c, depth + 1)));
        }
        out
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for t in self.tasks.values() {
            c.add(&t.status);
        }
        c
    }

    pub fn max_seq(&self) -> u64 {
        self.tasks.values().map(|t| t.seq).max().unwrap_or(0)
    }
}

#[derive(Default, Clone, Copy)]
pub struct Counts {
    pub todo: u64,
    pub doing: u64,
    pub blocked: u64,
    pub done: u64,
    pub dropped: u64,
}

impl Counts {
    pub fn add(&mut self, status: &str) {
        match status {
            "todo" => self.todo += 1,
            "doing" => self.doing += 1,
            "blocked" => self.blocked += 1,
            "done" => self.done += 1,
            _ => self.dropped += 1,
        }
    }

    pub fn total(&self) -> u64 {
        self.todo + self.doing + self.blocked + self.done + self.dropped
    }

    pub fn line(&self) -> String {
        format!(
            "{} todo, {} doing, {} blocked, {} done, {} dropped",
            self.todo, self.doing, self.blocked, self.done, self.dropped
        )
    }

    pub fn to_value(self) -> Value {
        Value::obj(vec![
            ("todo", Value::uint(self.todo)),
            ("doing", Value::uint(self.doing)),
            ("blocked", Value::uint(self.blocked)),
            ("done", Value::uint(self.done)),
            ("dropped", Value::uint(self.dropped)),
        ])
    }
}

/// `board/`, refused when it is a symlink. It may not exist yet.
pub fn board_dir(store: &Store) -> Result<PathBuf, String> {
    fsx::join_child(&store.dir, &["board"])
}

pub fn archive_dir(store: &Store) -> Result<PathBuf, String> {
    fsx::join_child(&board_dir(store)?, &["archive"])
}

pub fn task_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    Ok(fsx::join_id(dir, id, 1)?.with_extension("md"))
}

/// The live board. A missing board folder is an empty board. A damaged task
/// file is an error naming the file.
pub fn load(store: &Store) -> Result<Board, String> {
    let dir = board_dir(store)?;
    let (tasks, warnings) = read_folder(&dir, true)?;
    let mut b = Board::from_tasks(tasks)?;
    let mut all = warnings;
    all.append(&mut b.warnings);
    b.warnings = all;
    Ok(b)
}

/// One archived project: its folder name under `archive/` and its tasks.
pub struct Archived {
    pub name: String,
    pub board: Board,
}

/// Every archived project, oldest first by the project's `closed` time.
pub fn load_archive(store: &Store) -> Result<Vec<Archived>, String> {
    let dir = archive_dir(store)?;
    let rd = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    let mut out = Vec::new();
    for e in rd {
        let e = e.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let ft = e
            .file_type()
            .map_err(|err| format!("cannot stat {}: {err}", e.path().display()))?;
        if ft.is_symlink() {
            return Err(format!(
                "{} is a symlink; refusing to follow it",
                e.path().display()
            ));
        }
        if !ft.is_dir() || !valid_task_id(&name) {
            return Err(format!(
                "unexpected entry {} in the board archive",
                e.path().display()
            ));
        }
        let (tasks, _) = read_folder(&e.path(), false)?;
        let board = Board::from_tasks(tasks)
            .map_err(|err| format!("archive {}: {err}", e.path().display()))?;
        out.push(Archived { name, board });
    }
    let closed = |a: &Archived| {
        a.board
            .roots()
            .first()
            .and_then(|r| a.board.get(r))
            .and_then(|t| t.closed.clone())
            .unwrap_or_default()
    };
    out.sort_by(|a, b| closed(a).cmp(&closed(b)).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// Read every `<id>.md` in a board folder. `live` allows `seq` and
/// `archive/` beside the tasks.
fn read_folder(dir: &Path, live: bool) -> Result<(Vec<Task>, Vec<String>), String> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), Vec::new())),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    for e in rd {
        let e = e.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue; // locks and temp files
        }
        let ft = e
            .file_type()
            .map_err(|err| format!("cannot stat {}: {err}", path.display()))?;
        if ft.is_symlink() {
            return Err(format!(
                "{} is a symlink; refusing to follow it",
                path.display()
            ));
        }
        if live && ft.is_dir() && name == "archive" {
            continue;
        }
        if live && name == "seq" && !ft.is_dir() {
            continue;
        }
        if !ft.is_file() {
            if ft.is_dir() {
                return Err(format!(
                    "unexpected folder {} in the board folder",
                    path.display()
                ));
            }
            warnings.push(format!("skipped {}: not a regular file", path.display()));
            continue;
        }
        let Some(stem) = name.strip_suffix(".md") else {
            return Err(format!(
                "unexpected file {} in the board folder",
                path.display()
            ));
        };
        if !valid_task_id(stem) {
            warnings.push(format!(
                "skipped {}: \"{stem}\" is not a task id",
                path.display()
            ));
            continue;
        }
        files.push((stem.to_string(), path));
    }
    let read = |(id, path): &(String, PathBuf)| -> Result<Option<Task>, String> {
        match fsx::file_kind(path)? {
            fsx::FileKind::Missing => return Ok(None), // moved by a writer just now
            fsx::FileKind::Regular { len } if len > MAX_TASK_BYTES => {
                return Err(format!(
                    "task file {} is {len} bytes, over the {MAX_TASK_BYTES} byte cap",
                    path.display()
                ))
            }
            fsx::FileKind::Regular { .. } => {}
            _ => return Err(format!("{} is not a regular file", path.display())),
        }
        let text = match fs::read(path) {
            Ok(b) => String::from_utf8(b)
                .map_err(|_| format!("{} is not valid UTF-8", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        Task::parse(id, &text)
            .map(Some)
            .map_err(|e| format!("corrupt board task {id} ({}): {e}", path.display()))
    };
    let results: Vec<Result<Option<Task>, String>> = if files.len() < 128 {
        files.iter().map(read).collect()
    } else {
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(2, 8);
        let chunk = files.len().div_ceil(workers).max(1);
        std::thread::scope(|scope| {
            let handles: Vec<_> = files
                .chunks(chunk)
                .map(|part| scope.spawn(move || part.iter().map(read).collect::<Vec<_>>()))
                .collect();
            handles
                .into_iter()
                .flat_map(|h| {
                    h.join().unwrap_or_else(|_| {
                        vec![Err("internal error: a board reader panicked".to_string())]
                    })
                })
                .collect()
        })
    };
    let mut tasks = Vec::with_capacity(files.len());
    for r in results {
        if let Some(t) = r? {
            tasks.push(t);
        }
    }
    Ok((tasks, warnings))
}

/// What `libellus_board` and the brief show of one task.
pub fn task_line(t: &Task, caller: &str) -> String {
    let mut line = format!("{} {}: {}", mark(&t.status), t.id, t.title);
    if t.owner != caller {
        line.push_str(&format!(" @{}", t.owner));
    }
    if let Some(o) = &t.outcome {
        line.push_str(&format!(" -> {o}"));
    }
    line
}

/// The board as checkbox markdown.
pub struct Filter<'a> {
    pub owner: Option<&'a str>,
    pub project: Option<&'a str>,
    pub open_only: bool,
}

/// The ids to show under `filter`: matching tasks plus their parents, so
/// every line keeps its place in the tree.
pub fn shown(b: &Board, f: &Filter) -> std::collections::HashSet<String> {
    let mut keep = std::collections::HashSet::new();
    for t in b.tasks.values() {
        if f.open_only && !is_open(&t.status) {
            continue;
        }
        if f.owner.is_some_and(|o| t.owner != o) {
            continue;
        }
        let path = b.path(&t.id);
        if f.project
            .is_some_and(|p| path.first().map(String::as_str) != Some(p))
        {
            continue;
        }
        keep.extend(path);
    }
    keep
}

pub fn render_tree(
    b: &Board,
    keep: Option<&std::collections::HashSet<String>>,
    caller: &str,
) -> String {
    let mut out = String::new();
    for (t, depth) in b.tree_order() {
        if keep.is_some_and(|k| !k.contains(&t.id)) {
            continue;
        }
        out.push_str(&"  ".repeat(depth - 1));
        out.push_str("- ");
        out.push_str(&task_line(t, caller));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, parent: &str, status: &str) -> Task {
        let closed = !is_open(status);
        Task {
            id: id.into(),
            title: format!("Title {id}"),
            status: status.into(),
            parent: parent.into(),
            owner: "main".into(),
            created: "2026-11-26T00:00:00Z".into(),
            updated: "2026-11-26T00:00:00Z".into(),
            updated_by: "main".into(),
            seq: 1,
            closed: closed.then(|| "2026-11-26T00:00:00Z".to_string()),
            closed_by: closed.then(|| "main".to_string()),
            outcome: None,
            evidence: vec!["adsertio:c-1".into()],
        }
    }

    #[test]
    fn a_task_round_trips_and_parsing_is_strict() {
        let t = task("a", "", "todo");
        assert_eq!(Task::parse("a", &t.render()).unwrap(), t);
        let d = task("b", "a", "done");
        assert_eq!(Task::parse("b", &d.render()).unwrap(), d);
        let ok = t.render();
        assert!(Task::parse("a", &ok.replace("status: todo", "status: maybe")).is_err());
        assert!(Task::parse("a", &ok.replace("owner: main", "owner: main\ncolour: red")).is_err());
        assert!(Task::parse("a", &format!("{ok}body\n")).is_err());
        assert!(Task::parse("a", &ok.replace("parent:\n", "")).is_err());
        assert!(Task::parse("a", &ok.replace("parent:\n", "parent: a\n")).is_err());
        assert!(Task::parse("a", &ok.replace("status: todo", "status: done")).is_err());
        assert!(Task::parse("a", &ok.replace("owner: main", "owner: children")).is_err());
    }

    #[test]
    fn a_loop_is_damage() {
        let a = task("a", "b", "todo");
        let b = task("b", "a", "todo");
        assert!(Board::from_tasks(vec![a, b]).is_err());
        let orphan = task("c", "gone", "todo");
        let board = Board::from_tasks(vec![orphan]).unwrap();
        assert_eq!(board.roots(), &["c".to_string()]);
        assert_eq!(board.warnings.len(), 1);
    }
}
