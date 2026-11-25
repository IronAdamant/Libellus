//! The tool table: what an agent can do, over MCP or the CLI.

use crate::json::Value;
use crate::note::{self, Note};
use crate::promote;
use crate::store::Store;
use crate::tool::{Args, Kind, Param, Tool};

pub const GROUPS: &[&str] = &["core", "ledger", "board"];

/// An optional parameter. `doc` is empty for a batch item's fields.
const fn opt(name: &'static str, kind: Kind, doc: &'static str) -> Param {
    Param {
        name,
        kind,
        required: false,
        doc,
        choices: &[],
    }
}

/// A required parameter.
const fn req(name: &'static str, kind: Kind, doc: &'static str) -> Param {
    Param {
        name,
        kind,
        required: true,
        doc,
        choices: &[],
    }
}

/// A string parameter limited to `choices`, sent as a schema enum.
const fn one_of(
    name: &'static str,
    required: bool,
    doc: &'static str,
    choices: &'static [&'static str],
) -> Param {
    Param {
        name,
        kind: Kind::Str,
        required,
        doc,
        choices,
    }
}

const AGENT: Param = req("agent", Kind::Str, "Your agent name (main, sub-2).");

const NOTE_ID: Param = req("id", Kind::Str, "Note id.");

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "libellus_note",
        group: "core",
        description: "Write private notes as you work: decisions, dead ends, findings, open questions. Pass notes to write several in one call; one bad note writes nothing. A taken id fails.",
        params: &[
            AGENT,
            opt(
                "notes",
                Kind::Objects(NOTE_FIELDS, crate::note_cmd::MAX_BATCH),
                "Batch, instead of the single-note fields below.",
            ),
            one_of("kind", false, "Required without notes.", note::KINDS),
            opt(
                "summary",
                Kind::Str,
                "One line, up to 200 characters. Required without notes.",
            ),
            opt("body", Kind::Str, "Detail, under about 300 words."),
            opt("id", Kind::Str, "Slug, one or two segments (a/b). Default: from summary."),
            one_of(
                "status",
                false,
                "Default: open for a question, else resolved.",
                note::STATUSES,
            ),
            opt(
                "evidence",
                Kind::StrList,
                "Pointers as tool:reference (trammel:plan-3/step-2), not copies.",
            ),
            opt("tags", Kind::StrList, "Tag slugs."),
        ],
        run: crate::note_cmd::add,
    },
    Tool {
        name: "libellus_get",
        group: "core",
        description: "Read one of your notes in full, by id.",
        params: &[AGENT, NOTE_ID],
        run: note_get,
    },
    Tool {
        name: "libellus_resolve",
        group: "core",
        description: "Close notes: mark resolved, append a one-line outcome. Pass items to close several in one call; one bad item writes nothing. A resolved note changes only with supersede or an outcome starting SUPERSEDED: or REVERSED:.",
        params: &[
            AGENT,
            opt(
                "items",
                Kind::Objects(RESOLVE_FIELDS, crate::note_cmd::MAX_BATCH),
                "Batch, instead of id and outcome.",
            ),
            opt("id", Kind::Str, "Required without items."),
            opt(
                "outcome",
                Kind::Str,
                "One line, up to 200 characters. Required without items.",
            ),
            opt("supersede", Kind::Bool, "Mark the note replaced."),
        ],
        run: crate::note_cmd::resolve,
    },
    Tool {
        name: "libellus_ls",
        group: "core",
        description: "List your notes: the newest page, oldest first. total is the full count.",
        params: &[
            AGENT,
            one_of("kind", false, "", note::KINDS),
            one_of("status", false, "", note::STATUSES),
            opt("limit", Kind::UInt, "Default 50."),
            opt("offset", Kind::UInt, "Skip this many newest. Prefer before_id."),
            opt("before_id", Kind::Str, "Next page: next_before_id from the last one."),
            opt("bodies", Kind::Bool, "Include bodies, cut at 4000 characters."),
        ],
        run: note_ls,
    },
    Tool {
        name: "libellus_brief",
        group: "core",
        description: "Call at session start and after compaction, before other work. One call returns your open board tasks, open questions, recent decisions, dead ends, findings, costs and a short text.",
        params: &[
            AGENT,
            opt("max_items", Kind::UInt, "Per section. Default 5, at most 50."),
        ],
        run: brief,
    },
    Tool {
        name: "libellus_promote",
        group: "core",
        description: "Share one note with the swarm: write it into Relatum and record the new id. Resolve an open question first. A repeat call writes nothing.",
        params: &[
            AGENT,
            NOTE_ID,
            req("scope", Kind::Str, "Relatum scope (net, _shared)."),
        ],
        run: promote_note,
    },
    Tool {
        name: "libellus_spend",
        group: "ledger",
        description: "Record tokens and cost you already know for one step. Each call appends; Libellus only adds up.",
        params: &[
            AGENT,
            req("task", Kind::Str, "Task slug."),
            opt("step", Kind::Str, "Step slug."),
            opt("model", Kind::Str, ""),
            opt("tokens_in", Kind::UInt, ""),
            opt("tokens_out", Kind::UInt, ""),
            opt("cost_micro_usd", Kind::UInt, "Millionths of a US dollar."),
            opt("note", Kind::Str, "One line."),
        ],
        run: spend,
    },
    Tool {
        name: "libellus_ledger",
        group: "ledger",
        description: "Totals of tokens and cost, by task. Narrow by agent, task or step. archive moves one agent's ledger aside and starts a fresh one, when spend says it is full.",
        params: &[
            opt("agent", Kind::Str, "Omit to sum every agent."),
            opt("task", Kind::Str, ""),
            opt("step", Kind::Str, ""),
            opt("archive", Kind::Bool, "With agent only."),
        ],
        run: ledger_report,
    },
    Tool {
        name: "libellus_doctor",
        group: "core",
        description: "Diagnose the setup and read every note, ledger and board file for damage. Returns ok and checks [{name, ok, detail, fix}]. Read-only unless fix.",
        params: &[opt(
            "fix",
            Kind::Bool,
            "Remove locks and temp files whose writer is gone.",
        )],
        run: crate::doctor::run,
    },
    Tool {
        name: "libellus_plan",
        group: "board",
        description: "Change the shared task board, every agent's working list, in one call. ops (applied in order): {add:{title,...}}, {set:{id,...}}, {remove:id}. outline: a markdown checkbox tree, two spaces per level: - [ ] todo, [~] doing, [!] blocked, [x] done, [-] dropped. One bad op writes nothing. A parent closes when its children are done or dropped; a task with an open child cannot be done. Tick several tasks per call.",
        params: &[
            AGENT,
            opt("ops", Kind::Objects(OP_FIELDS, crate::plan::MAX_OPS), ""),
            opt("outline", Kind::Str, "Ids come back in added."),
            opt("parent", Kind::Str, "Task the outline goes under. Omit for projects."),
        ],
        run: crate::plan::run,
    },
    Tool {
        name: "libellus_board",
        group: "board",
        description: "Read the shared task board as a checkbox tree with counts per status. Open tasks only by default.",
        params: &[
            AGENT,
            opt("owner", Kind::Str, ""),
            opt("project", Kind::Str, "Top-level task id."),
            opt("open_only", Kind::Bool, "Default true."),
            opt("include_archived", Kind::Bool, ""),
        ],
        run: crate::board_view::run,
    },
];

// Batch item fields carry no docs: the single form and the parent's
// description already say what each field is.

const NOTE_FIELDS: &[Param] = &[
    one_of("kind", true, "", note::KINDS),
    req("summary", Kind::Str, ""),
    opt("body", Kind::Str, ""),
    opt("id", Kind::Str, ""),
    one_of("status", false, "", note::STATUSES),
    opt("evidence", Kind::StrList, ""),
    opt("tags", Kind::StrList, ""),
];

const RESOLVE_FIELDS: &[Param] = &[
    req("id", Kind::Str, ""),
    req("outcome", Kind::Str, ""),
    opt("supersede", Kind::Bool, ""),
];

const ADD_FIELDS: &[Param] = &[
    req("title", Kind::Str, ""),
    opt("id", Kind::Str, ""),
    opt("parent", Kind::Str, "Omit for a project."),
    opt("owner", Kind::Str, "Default: you."),
    one_of("status", false, "", crate::board::STATUSES),
];

const SET_FIELDS: &[Param] = &[
    req("id", Kind::Str, ""),
    one_of("status", false, "", crate::board::STATUSES),
    opt("title", Kind::Str, ""),
    opt("parent", Kind::Str, "\"\" makes it a project."),
    opt("owner", Kind::Str, ""),
    opt("outcome", Kind::Str, "One line. \"\" clears it."),
    opt(
        "evidence",
        Kind::StrList,
        "tool:reference. Replaces the list.",
    ),
];

const OP_FIELDS: &[Param] = &[
    opt("add", Kind::Object(ADD_FIELDS), ""),
    opt("set", Kind::Object(SET_FIELDS), ""),
    opt(
        "remove",
        Kind::Str,
        "Delete a childless todo task. Else set status dropped.",
    ),
];

fn note_get(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    let id = a.str("id").unwrap_or_default();
    Ok(note_full(&note::load_one(&store, agent, id)?))
}

/// Every field of one note. The body is not cut: this is a single note, not a list.
pub fn note_full(n: &Note) -> Value {
    let mut p = vec![
        ("id", Value::str(&n.id)),
        ("kind", Value::str(&n.kind)),
        ("status", Value::str(&n.status)),
        ("summary", Value::str(&n.summary)),
        ("agent", Value::str(&n.agent)),
        ("created", Value::str(&n.created)),
        (
            "evidence",
            Value::Array(n.evidence.iter().map(Value::str).collect()),
        ),
        (
            "tags",
            Value::Array(n.tags.iter().map(Value::str).collect()),
        ),
        ("body", Value::str(&n.body)),
    ];
    if let Some(seq) = n.seq {
        p.push(("seq", Value::uint(seq)));
    }
    if n.superseded {
        p.push(("superseded", Value::Bool(true)));
    }
    if let Some(pr) = &n.promoted {
        p.push(("promoted", Value::str(pr)));
    }
    Value::obj(p)
}

fn note_value(n: &Note, body: bool) -> Value {
    let mut p = vec![
        ("id", Value::str(&n.id)),
        ("kind", Value::str(&n.kind)),
        ("status", Value::str(&n.status)),
        ("summary", Value::str(&n.summary)),
        ("created", Value::str(&n.created)),
    ];
    if let Some(seq) = n.seq {
        p.push(("seq", Value::uint(seq)));
    }
    if n.superseded {
        p.push(("superseded", Value::Bool(true)));
    }
    if !n.evidence.is_empty() {
        p.push((
            "evidence",
            Value::Array(n.evidence.iter().map(Value::str).collect()),
        ));
    }
    if let Some(pr) = &n.promoted {
        p.push(("promoted", Value::str(pr)));
    }
    if body {
        let (text, cut) = if n.body.chars().count() > 4000 {
            (n.body.chars().take(4000).collect(), true)
        } else {
            (n.body.clone(), false)
        };
        p.push(("body", Value::String(text)));
        if cut {
            p.push(("truncated", Value::Bool(true)));
        }
    }
    Value::obj(p)
}

fn note_ls(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let loaded = note::load_all(&store, a.str("agent").unwrap_or_default())?;
    let all = loaded.notes;
    let keep = |n: &Note| {
        a.str("kind").is_none_or(|k| n.kind == k) && a.str("status").is_none_or(|s| n.status == s)
    };
    // Each kept note with its place in the full list.
    let filtered: Vec<(usize, &Note)> = all.iter().enumerate().filter(|(_, n)| keep(n)).collect();
    let total = filtered.len();
    let limit = a.uint("limit").unwrap_or(50) as usize;
    let offset = a.uint("offset").unwrap_or(0) as usize;
    // before_id is the stable cursor. It is placed by the list's own order
    // (seq, or created then id), not by finding it in this filtered list, so
    // it still works after the cursor note was resolved, re-filtered or
    // deleted. `older` is how many filtered notes sort before the cursor.
    let older = match a.str("before_id") {
        Some(cursor) => {
            let rank = cursor_rank(&all, cursor)?;
            filtered.iter().take_while(|(at, _)| *at < rank).count()
        }
        None => total,
    };
    // Newest notes are at the end. offset skips that many, then limit keeps
    // the newest of what remains. offset 0 with no before_id is the 1.0.0 page.
    let end = older.saturating_sub(offset);
    let start = end.saturating_sub(limit);
    let notes: Vec<&Note> = filtered[start..end].iter().map(|(_, n)| *n).collect();
    let truncated = start > 0;
    let returned = notes.len() as u64;
    let next_before = match notes.first() {
        Some(oldest) if truncated => Value::String(cursor_of(oldest)),
        _ => Value::Null,
    };
    // next_offset is absolute: the offset to pass without before_id for the
    // next older page, counted from the newest note in this filtered list.
    let next_offset = if truncated {
        Value::uint((total - start) as u64)
    } else {
        Value::Null
    };
    let bodies = a.bool("bodies").unwrap_or(false);
    let mut pairs = vec![
        (
            "notes".to_string(),
            Value::Array(notes.iter().map(|n| note_value(n, bodies)).collect()),
        ),
        ("total".to_string(), Value::uint(total as u64)),
        ("offset".to_string(), Value::uint(offset as u64)),
        ("returned".to_string(), Value::uint(returned)),
        ("truncated".to_string(), Value::Bool(truncated)),
        ("next_offset".to_string(), next_offset),
        ("next_before_id".to_string(), next_before),
    ];
    if !loaded.warnings.is_empty() {
        pairs.push((
            "warnings".to_string(),
            Value::Array(loaded.warnings.iter().map(Value::str).collect()),
        ));
    }
    Ok(Value::Object(pairs))
}

/// The cursor handed out as `next_before_id`: the note id, then `@` and its
/// `created` time, then `@` and its `seq` when it has one. The id alone is
/// also accepted while that note still exists.
fn cursor_of(n: &Note) -> String {
    match n.seq {
        Some(seq) => format!("{}@{}@{seq}", n.id, n.created),
        None => format!("{}@{}", n.id, n.created),
    }
}

/// How many notes of the full list sort before the cursor. A cursor that
/// names a stored note uses that note's place. A cursor whose note is gone is
/// placed by its key, by merging a stand-in note into the list order.
fn cursor_rank(all: &[Note], cursor: &str) -> Result<usize, String> {
    let mut parts = cursor.split('@');
    let id = parts.next().unwrap_or_default();
    let created = parts.next();
    let seq = parts.next();
    let bad = || {
        format!(
            "before_id \"{}\" is not a cursor. Pass next_before_id from the previous page, or a note id.",
            crate::fsx::clip(cursor, 80)
        )
    };
    if parts.next().is_some() || id.is_empty() {
        return Err(bad());
    }
    if let Some(at) = all.iter().position(|n| n.id == id) {
        return Ok(at);
    }
    let Some(created) = created else {
        return Err(format!(
            "before_id \"{}\" names no note. Pass next_before_id from the previous page, which still works after that note is deleted.",
            crate::fsx::clip(id, 80)
        ));
    };
    if !crate::fsx::valid_timestamp(created) {
        return Err(bad());
    }
    let seq = match seq {
        Some(v) => Some(note::parse_seq(v).map_err(|_| bad())?),
        None => None,
    };
    let mut merged: Vec<Note> = all.to_vec();
    merged.push(Note {
        id: id.to_string(),
        kind: String::new(),
        status: String::new(),
        summary: String::new(),
        agent: String::new(),
        created: created.to_string(),
        evidence: Vec::new(),
        tags: Vec::new(),
        promoted: None,
        seq,
        superseded: false,
        body: String::new(),
    });
    note::sort_notes(&mut merged);
    Ok(merged
        .iter()
        .position(|n| n.id == id && n.kind.is_empty())
        .unwrap_or(merged.len()))
}

fn brief(a: &Args) -> Result<Value, String> {
    crate::brief::run(a)
}

fn promote_note(a: &Args) -> Result<Value, String> {
    promote::promote(
        a.str("agent").unwrap_or_default(),
        a.str("id").unwrap_or_default(),
        a.str("scope").unwrap_or_default(),
    )
}

fn spend(a: &Args) -> Result<Value, String> {
    crate::ledger_cmd::spend(a)
}

/// Totals, or with `archive` set, move one agent's ledger aside. The agent
/// is optional for totals, so `LIBELLUS_AGENT` fills it only for archive.
fn ledger_report(a: &Args) -> Result<Value, String> {
    if a.bool("archive") != Some(true) {
        return crate::ledger_cmd::report(a);
    }
    if a.has("task") || a.has("step") {
        return Err("archive takes only agent; drop task and step".into());
    }
    let agent = match a.str("agent") {
        Some(name) => name.to_string(),
        None => std::env::var("LIBELLUS_AGENT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or("archive needs agent. Pass agent, or set LIBELLUS_AGENT.")?,
    };
    crate::ledger_cmd::archive(&agent)
}
