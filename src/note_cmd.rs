//! `libellus_note` and `libellus_resolve`, one at a time or in a batch.
//!
//! A batch is checked whole before anything is written: one bad item means
//! no file changes, and the error names every bad item. Then the items are
//! written in order. A write that fails part way stops the batch, and the
//! error names the items already written.

use crate::fsx;
use crate::json::Value;
use crate::note::{self, Note};
use crate::store::{Locked, Store};
use crate::tool::{field, field_list, field_str, Args};

/// Most notes or resolves in one call.
pub const MAX_BATCH: usize = 50;

/// One note to write, checked.
struct NewNote {
    id: Option<String>,
    kind: String,
    status: String,
    summary: String,
    body: String,
    evidence: Vec<String>,
    tags: Vec<String>,
}

const SINGLE: &[&str] = &[
    "kind", "summary", "body", "id", "status", "evidence", "tags",
];

pub fn add(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    let notes_dir = store.notes_dir(agent)?;
    let Some(items) = a.objects("notes") else {
        let kind = a
            .str("kind")
            .ok_or("missing required argument: kind (or pass notes, a batch)")?;
        let summary = a
            .str("summary")
            .ok_or("missing required argument: summary (or pass notes, a batch)")?;
        let new = check_new(
            kind,
            a.str("status"),
            summary,
            a.str("body"),
            a.str("id"),
            a.list("evidence").iter().map(|s| s.to_string()).collect(),
            a.list("tags").iter().map(|s| s.to_string()).collect(),
        )?;
        let seq = crate::seq::alloc_seqs(&store, agent, 1)?;
        let (id, status) = write_new(&notes_dir, agent, &new, seq)?;
        return Ok(Value::obj(vec![
            ("id", Value::String(id)),
            ("kind", Value::String(new.kind)),
            ("status", Value::String(status)),
        ]));
    };
    if let Some(extra) = SINGLE.iter().find(|k| a.has(k)) {
        return Err(format!(
            "pass either notes (a batch) or one note's fields, not both: {extra} was given with notes"
        ));
    }
    if items.is_empty() {
        return Err("notes is empty; pass at least one note".into());
    }
    let mut batch = Vec::new();
    let mut bad = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let made = check_new(
            field_str(item, "kind").unwrap_or_default(),
            field_str(item, "status"),
            field_str(item, "summary").unwrap_or_default(),
            field_str(item, "body"),
            field_str(item, "id"),
            field_list(item, "evidence").unwrap_or_default(),
            field_list(item, "tags").unwrap_or_default(),
        )
        .and_then(|n| {
            if let Some(id) = &n.id {
                if batch.iter().any(|o: &NewNote| o.id.as_deref() == Some(id)) {
                    return Err(format!("id \"{id}\" appears twice in this batch"));
                }
                let path = note::note_path(&store, agent, id)?;
                if fsx::path_exists_nofollow(&path) {
                    return Err(format!(
                        "note \"{id}\" already exists. Read it with libellus_get, or pass a new id."
                    ));
                }
            }
            Ok(n)
        });
        match made {
            Ok(n) => batch.push(n),
            Err(e) => bad.push(format!("notes[{i}]: {e}")),
        }
    }
    if !bad.is_empty() {
        return Err(format!(
            "nothing written: {} of {} notes are invalid. {}",
            bad.len(),
            items.len(),
            bad.join("; ")
        ));
    }
    let first = crate::seq::alloc_seqs(&store, agent, batch.len() as u64)?;
    let mut written = Vec::new();
    for (i, new) in batch.iter().enumerate() {
        match write_new(&notes_dir, agent, new, first + i as u64) {
            Ok((id, _)) => written.push(id),
            Err(e) => return Err(partial("notes", "written", &written, i, batch.len(), &e)),
        }
    }
    Ok(Value::obj(vec![
        ("written", Value::uint(written.len() as u64)),
        (
            "ids",
            Value::Array(written.into_iter().map(Value::String).collect()),
        ),
    ]))
}

/// The error for a batch that stopped part way.
fn partial(what: &str, verb: &str, done: &[String], at: usize, total: usize, e: &str) -> String {
    let list = if done.is_empty() {
        "none".to_string()
    } else {
        done.join(", ")
    };
    let rest = if at + 1 < total {
        format!(" {what}[{}..{}] were not tried.", at + 1, total - 1)
    } else {
        String::new()
    };
    format!(
        "batch stopped at {what}[{at}]: {e}. {verb} {} of {total}: {list}.{rest}",
        done.len()
    )
}

#[allow(clippy::too_many_arguments)]
fn check_new(
    kind: &str,
    status: Option<&str>,
    summary: &str,
    body: Option<&str>,
    id: Option<&str>,
    evidence: Vec<String>,
    tags: Vec<String>,
) -> Result<NewNote, String> {
    if !note::KINDS.contains(&kind) {
        return Err(format!("kind must be one of: {}", note::KINDS.join(", ")));
    }
    if let Some(s) = status {
        if !note::STATUSES.contains(&s) {
            return Err(format!(
                "status must be one of: {}",
                note::STATUSES.join(", ")
            ));
        }
    }
    note::validate_summary(summary)?;
    if let Some(bad) = evidence.iter().find(|p| !note::valid_pointer(p)) {
        return Err(format!(
            "bad evidence pointer \"{}\": use tool:reference with no spaces, commas or brackets",
            fsx::clip(bad, 60)
        ));
    }
    if let Some(bad) = tags.iter().find(|t| !fsx::valid_slug(t)) {
        return Err(format!("bad tag \"{}\": use a slug", fsx::clip(bad, 40)));
    }
    let mut seen = Vec::new();
    for tag in tags {
        if !seen.contains(&tag) {
            seen.push(tag);
        }
    }
    if let Some(id) = id {
        if !note::valid_note_id(id) {
            // join_id gives the full reason, including device names.
            fsx::join_id(std::path::Path::new("."), id, 2)?;
            return Err(format!("invalid id \"{}\"", fsx::clip(id, 60)));
        }
    }
    Ok(NewNote {
        id: id.map(str::to_string),
        kind: kind.to_string(),
        status: status
            .unwrap_or(if kind == "question" {
                "open"
            } else {
                "resolved"
            })
            .to_string(),
        summary: summary.to_string(),
        body: body.unwrap_or_default().to_string(),
        evidence,
        tags: seen,
    })
}

/// Write one new note with `overwrite = false`. An id made from the summary
/// gains `-2`, `-3` and so on when taken; a given id that is taken fails.
fn write_new(
    notes_dir: &std::path::Path,
    agent: &str,
    new: &NewNote,
    seq: u64,
) -> Result<(String, String), String> {
    let attempts: u32 = if new.id.is_some() { 1 } else { 999 };
    for ntry in 1..=attempts {
        let id = match &new.id {
            Some(id) => id.clone(),
            None => fsx::slug_from(&new.summary, ntry, "note"),
        };
        let path = fsx::join_id(notes_dir, &id, 2)?.with_extension("md");
        if let Some(dir) = path.parent() {
            fsx::make_private_dir(dir)?;
        }
        fsx::refuse_special(&path)?;
        let n = Note {
            id: id.clone(),
            kind: new.kind.clone(),
            status: new.status.clone(),
            summary: new.summary.clone(),
            agent: agent.to_string(),
            created: fsx::now_iso(),
            evidence: new.evidence.clone(),
            tags: new.tags.clone(),
            promoted: None,
            seq: Some(seq),
            superseded: false,
            body: new.body.clone(),
        };
        match fsx::write_atomic(&path, n.render().as_bytes(), false) {
            Ok(()) => return Ok((id, n.status)),
            Err(e) if new.id.is_none() && e.contains("already exists") => continue,
            Err(e) if e.contains("already exists") => {
                return Err(format!(
                    "note \"{id}\" already exists. Read it with libellus_get, or pass a new id."
                ));
            }
            Err(e) => return Err(format!("note \"{id}\" not written: {e}")),
        }
    }
    Err("could not find a free id; pass id".into())
}

const SINGLE_RESOLVE: &[&str] = &["id", "outcome", "supersede"];

pub fn resolve(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    let Some(items) = a.objects("items") else {
        let id = a
            .str("id")
            .ok_or("missing required argument: id (or pass items, a batch)")?;
        let outcome = a
            .str("outcome")
            .ok_or("missing required argument: outcome (or pass items, a batch)")?;
        note::validate_outcome(outcome)?;
        let n = resolve_one(&store, agent, id, outcome, a.bool("supersede"))?;
        return Ok(crate::tools::note_full(&n));
    };
    if let Some(extra) = SINGLE_RESOLVE.iter().find(|k| a.has(k)) {
        return Err(format!(
            "pass either items (a batch) or one id and outcome, not both: {extra} was given with items"
        ));
    }
    if items.is_empty() {
        return Err("items is empty; pass at least one {id, outcome}".into());
    }
    let mut bad = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let id = field_str(item, "id").unwrap_or_default();
        let outcome = field_str(item, "outcome").unwrap_or_default();
        let supersede = field(item, "supersede").and_then(Value::as_bool);
        let check = (|| -> Result<(), String> {
            if seen.contains(&id) {
                return Err(format!("id \"{id}\" appears twice in this batch"));
            }
            note::validate_outcome(outcome)?;
            let n = note::load_one(&store, agent, id)?;
            if n.status == "resolved" && !wants_supersede(outcome, supersede) {
                return Err(already_resolved(id));
            }
            Ok(())
        })();
        seen.push(id);
        if let Err(e) = check {
            bad.push(format!("items[{i}]: {e}"));
        }
    }
    if !bad.is_empty() {
        return Err(format!(
            "nothing written: {} of {} items are invalid. {}",
            bad.len(),
            items.len(),
            bad.join("; ")
        ));
    }
    let mut done = Vec::new();
    let mut rows = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let id = field_str(item, "id").unwrap_or_default();
        let outcome = field_str(item, "outcome").unwrap_or_default();
        let supersede = field(item, "supersede").and_then(Value::as_bool);
        match resolve_one(&store, agent, id, outcome, supersede) {
            Ok(n) => {
                let mut row = vec![("id", Value::str(&n.id)), ("status", Value::str(&n.status))];
                if n.superseded {
                    row.push(("superseded", Value::Bool(true)));
                }
                rows.push(Value::obj(row));
                done.push(n.id);
            }
            Err(e) => return Err(partial("items", "resolved", &done, i, items.len(), &e)),
        }
    }
    Ok(Value::obj(vec![
        ("resolved", Value::uint(rows.len() as u64)),
        ("notes", Value::Array(rows)),
    ]))
}

fn wants_supersede(outcome: &str, flag: Option<bool>) -> bool {
    flag.unwrap_or(false) || note::marks_replaced(outcome)
}

fn already_resolved(id: &str) -> String {
    format!(
        "note \"{id}\" is already resolved. The file was not changed. Read it with libellus_get. To mark it replaced, pass supersede or an outcome that starts with SUPERSEDED: or REVERSED:."
    )
}

/// Resolve one note under its lock: read, check, rewrite.
fn resolve_one(
    store: &Store,
    agent: &str,
    id: &str,
    outcome: &str,
    supersede: Option<bool>,
) -> Result<Note, String> {
    let path = note::note_path(store, agent, id)?;
    let _lock = if path.parent().is_some_and(|parent| parent.is_dir()) {
        Some(store.lock(Locked::Notes(agent))?)
    } else {
        None
    };
    let mut n = note::load_one(store, agent, id)?;
    let supersede = wants_supersede(outcome, supersede);
    if n.status == "resolved" && !supersede {
        return Err(already_resolved(id));
    }
    n.status = "resolved".to_string();
    if supersede {
        n.superseded = true;
    }
    n.body = note::append_outcome(&n.body, outcome);
    fsx::write_atomic(&path, n.render().as_bytes(), true)
        .map_err(|e| format!("note \"{id}\" not updated: {e}"))?;
    Ok(n)
}
