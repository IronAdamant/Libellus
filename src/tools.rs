//! The tool table: what an agent can do, over MCP or the CLI.

use crate::json::Value;
use crate::note::{self, Note};
use crate::promote;
use crate::store::Store;
use crate::tool::{Args, Kind, Param, Tool};

pub const GROUPS: &[&str] = &["core", "ledger", "board"];

const AGENT: Param = Param {
    name: "agent",
    kind: Kind::Str,
    required: true,
    doc: "Your own agent name: lowercase letters, digits, '-' (main, sub-2). Notes are private to it.",
    choices: &[],
};

const NOTE_ID: Param = Param {
    name: "id",
    kind: Kind::Str,
    required: true,
    doc: "Note id, one or two slug segments (retry/post-question).",
    choices: &[],
};

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "libellus_note",
        group: "core",
        description: "Write private notes before you forget a decision, a dead end, or an open question. Batch: pass notes (up to 50) to write several in one call, which costs one turn instead of one per note. A batch is checked whole first: one bad note writes nothing and the error names it. Or pass one note's kind and summary. Never overwrites: a taken id fails. Point at evidence as tool:reference instead of copying it.",
        params: &[
            AGENT,
            Param { name: "notes", kind: Kind::Objects(NOTE_FIELDS, crate::note_cmd::MAX_BATCH), required: false, doc: "A batch of up to 50 notes, each {kind, summary, body?, id?, status?, evidence?, tags?}. Use this instead of the single-note fields.", choices: &[] },
            Param { name: "kind", kind: Kind::Str, required: false, doc: "One note: what it is. Required without notes.", choices: note::KINDS },
            Param { name: "summary", kind: Kind::Str, required: false, doc: "One note: one line, up to 200 characters. Required without notes.", choices: &[] },
            Param { name: "body", kind: Kind::Str, required: false, doc: "The detail, under about 300 words, at most 20000 bytes.", choices: &[] },
            Param { name: "id", kind: Kind::Str, required: false, doc: "Slug, up to two segments (retry/post-question). Made from the summary if absent. Never overwrites.", choices: &[] },
            Param { name: "status", kind: Kind::Str, required: false, doc: "open or resolved. Default: open for a question, resolved otherwise.", choices: note::STATUSES },
            Param { name: "evidence", kind: Kind::StrList, required: false, doc: "Pointers to proof, each tool:reference (trammel:plan-3/step-2). No copies.", choices: &[] },
            Param { name: "tags", kind: Kind::StrList, required: false, doc: "Tag slugs.", choices: &[] },
        ],
        run: crate::note_cmd::add,
    },
    Tool {
        name: "libellus_get",
        group: "core",
        description: "Read one of your notes by id, including its body. Use this when you know the id. Missing, corrupt, or another agent's file is an error that says how to list or create the note.",
        params: &[AGENT, NOTE_ID],
        run: note_get,
    },
    Tool {
        name: "libellus_resolve",
        group: "core",
        description: "Mark notes resolved and append a one-line outcome to each. Batch: pass items (up to 50 of {id, outcome, supersede?}) to close several in one call; one bad item writes nothing. A note that is already resolved is left unchanged unless supersede is set, or the outcome starts with SUPERSEDED: or REVERSED:.",
        params: &[
            AGENT,
            Param { name: "items", kind: Kind::Objects(RESOLVE_FIELDS, crate::note_cmd::MAX_BATCH), required: false, doc: "A batch of up to 50 {id, outcome, supersede?}. Use this instead of id and outcome.", choices: &[] },
            Param { name: "id", kind: Kind::Str, required: false, doc: "One note: its id. Required without items.", choices: &[] },
            Param {
                name: "outcome",
                kind: Kind::Str,
                required: false,
                doc: "One note: one line, up to 200 characters, appended to the body. SUPERSEDED: or REVERSED: marks the note replaced. Required without items.",
                choices: &[],
            },
            Param {
                name: "supersede",
                kind: Kind::Bool,
                required: false,
                doc: "Mark the note replaced. Allowed when it is already resolved.",
                choices: &[],
            },
        ],
        run: crate::note_cmd::resolve,
    },
    Tool {
        name: "libellus_ls",
        group: "core",
        description: "List your notes, oldest first within the page. limit (default 50) keeps the newest page. offset skips that many newest notes so the next call can read the older page. total is the full count.",
        params: &[
            AGENT,
            Param { name: "kind", kind: Kind::Str, required: false, doc: "Only this kind.", choices: note::KINDS },
            Param { name: "status", kind: Kind::Str, required: false, doc: "Only this status.", choices: note::STATUSES },
            Param { name: "limit", kind: Kind::UInt, required: false, doc: "Most notes to return, newest kept. Default 50. Minimum 1.", choices: &[] },
            Param { name: "offset", kind: Kind::UInt, required: false, doc: "Skip this many of the newest matching notes (newer than before_id, when given) before applying limit. Default 0. next_offset is absolute: pass it without before_id. Prefer before_id: a note written between pages can show up twice with offset.", choices: &[] },
            Param { name: "before_id", kind: Kind::Str, required: false, doc: "Stable cursor: only notes older than this one in list order (seq, or created then id). Pass next_before_id from the previous page; it still works after that note is resolved or deleted. A note id is also accepted while the note exists.", choices: &[] },
            Param { name: "bodies", kind: Kind::Bool, required: false, doc: "Include each body (cut at 4000 characters).", choices: &[] },
        ],
        run: note_ls,
    },
    Tool {
        name: "libellus_brief",
        group: "core",
        description: "Call this when a session starts and after compaction, before other work: it is the one call you need to pick up again. Returns your open board tasks (with their path in the tree), open questions, the latest context, recent decisions, replaced decisions, dead ends, findings, costs, and a short text. max_items caps every section (default 5, at most 50). The text is capped at 8192 bytes and says when it was cut. A ledger that cannot be read sets ledger_error and still returns the notes. A Next line is added only when a question is open or a decision or dead end is not promoted.",
        params: &[
            AGENT,
            Param { name: "max_items", kind: Kind::UInt, required: false, doc: "Most notes per section, including open questions and task costs. Default 5. At most 50. Newest items are kept.", choices: &[] },
        ],
        run: brief,
    },
    Tool {
        name: "libellus_promote",
        group: "core",
        description: "Share one private note by writing it into Relatum, then record that id. An open question must be resolved first. A second call names the existing Relatum id and writes nothing. Does not run Relatum, and does not create .swarmnotes/.",
        params: &[
            AGENT,
            NOTE_ID,
            Param {
                name: "scope",
                kind: Kind::Str,
                required: true,
                doc: "Relatum scope, such as net or _shared. The new id is the scope plus this note's id.",
                choices: &[],
            },
        ],
        run: promote_note,
    },
    Tool {
        name: "libellus_spend",
        group: "ledger",
        description: "Append tokens and cost you already know for one step. Libellus only adds them up. Each call adds a line; it does not replace a previous spend.",
        params: &[
            AGENT,
            Param { name: "task", kind: Kind::Str, required: true, doc: "Task slug the spend belongs to.", choices: &[] },
            Param { name: "step", kind: Kind::Str, required: false, doc: "Step slug within the task.", choices: &[] },
            Param { name: "model", kind: Kind::Str, required: false, doc: "Model name, free text.", choices: &[] },
            Param { name: "tokens_in", kind: Kind::UInt, required: false, doc: "Input tokens.", choices: &[] },
            Param { name: "tokens_out", kind: Kind::UInt, required: false, doc: "Output tokens.", choices: &[] },
            Param { name: "cost_micro_usd", kind: Kind::UInt, required: false, doc: "Cost in millionths of a US dollar (1 cent = 10000).", choices: &[] },
            Param { name: "note", kind: Kind::Str, required: false, doc: "One line, up to 200 characters.", choices: &[] },
        ],
        run: spend,
    },
    Tool {
        name: "libellus_ledger",
        group: "ledger",
        description: "Totals of tokens and cost. Pass agent, task, or step to narrow. Omit agent to sum every agent in the store. A damaged ledger is an error, names the file, and is not repaired. by_task lists every task. The brief caps that list.",
        params: &[
            Param {
                name: "agent",
                kind: Kind::Str,
                required: false,
                doc: "Agent to sum. Omit to sum every agent in the store.",
                choices: &[],
            },
            Param { name: "task", kind: Kind::Str, required: false, doc: "Only this task slug.", choices: &[] },
            Param { name: "step", kind: Kind::Str, required: false, doc: "Only this step slug.", choices: &[] },
        ],
        run: ledger_report,
    },
    Tool {
        name: "libellus_archive",
        group: "ledger",
        description: "Move this agent's ledger into archive/ and start a fresh ledger. The old file is kept and is not included in later totals. Use this when a spend says the ledger is over the 32 MiB cap.",
        params: &[AGENT],
        run: archive_ledger,
    },
    Tool {
        name: "libellus_check",
        group: "core",
        description: "Read every note, ledger and board task. Fails if a file is damaged, the board is out of step (an open child under a closed parent, an orphan), a note is over the size cap, a total overflows, a note's agent field does not match its folder, a ledger line was left out, or a stale lock is left behind. Stale locks and leftover temp files are removed first.",
        params: &[],
        run: check_tool,
    },
    Tool {
        name: "libellus_doctor",
        group: "core",
        description: "Diagnose the setup: version, store location, LIBELLUS_DIR and LIBELLUS_AGENT, whether the store is writable, damaged files, abandoned locks, the AGENTS.md tool-groups line, libellus-mcp beside the binary, the Relatum store. Returns ok and checks [{name, ok, detail, fix}]. Read-only unless fix is set, which only clears abandoned locks and temp files.",
        params: &[Param { name: "fix", kind: Kind::Bool, required: false, doc: "Remove locks and temp files whose writer is gone, as check does. Nothing else is changed.", choices: &[] }],
        run: crate::doctor::run,
    },
    Tool {
        name: "libellus_plan",
        group: "board",
        description: "Change the shared task board in one call: the board is every agent's working task list (notes stay private). ops is a batch of up to 100 {add: {title, id?, parent?, owner?, status?}}, {set: {id, status?, title?, parent?, owner?, outcome?, evidence?}} or {remove: id}. outline adds a whole tree from a markdown checkbox list (\"- [ ] Project\\n  - [ ] Task\"; [x] done, [~] doing, [!] blocked, [-] dropped), under parent if given. Everything is checked first; one bad op writes nothing. A parent closes by itself when all its children are done or dropped (at least one done), up to the project, and a closed project moves to the archive. Dropping a task drops its open children. Reopening a child reopens parents that closed by themselves. A task cannot be done while a child is open. Batch your ticks: one call for several tasks.",
        params: &[
            AGENT,
            Param { name: "ops", kind: Kind::Objects(OP_FIELDS, crate::plan::MAX_OPS), required: false, doc: "Up to 100 ops, applied in order: {add: {...}}, {set: {...}} or {remove: id}.", choices: &[] },
            Param { name: "outline", kind: Kind::Str, required: false, doc: "A markdown checkbox list, two spaces or a tab per level: - [ ] todo, - [~] doing, - [!] blocked, - [x] done, - [-] dropped. Ids are made from the titles and returned in added.", choices: &[] },
            Param { name: "parent", kind: Kind::Str, required: false, doc: "Task id the outline goes under. Omit to make its top lines projects.", choices: &[] },
        ],
        run: crate::plan::run,
    },
    Tool {
        name: "libellus_board",
        group: "board",
        description: "Read the shared task board as a checkbox tree (ids shown, @owner when not you) plus counts per status. Open tasks only by default. Filter by owner or project. The brief already lists your own open tasks; call this for the whole picture.",
        params: &[
            AGENT,
            Param { name: "owner", kind: Kind::Str, required: false, doc: "Only tasks owned by this agent (with their parents for context).", choices: &[] },
            Param { name: "project", kind: Kind::Str, required: false, doc: "Only this project (a top-level task id).", choices: &[] },
            Param { name: "open_only", kind: Kind::Bool, required: false, doc: "Only todo, doing and blocked tasks. Default true.", choices: &[] },
            Param { name: "include_archived", kind: Kind::Bool, required: false, doc: "Also show archived (closed) projects.", choices: &[] },
        ],
        run: crate::board_view::run,
    },
];

const NOTE_FIELDS: &[Param] = &[
    Param {
        name: "kind",
        kind: Kind::Str,
        required: true,
        doc: "What the note is.",
        choices: note::KINDS,
    },
    Param {
        name: "summary",
        kind: Kind::Str,
        required: true,
        doc: "One line, up to 200 characters.",
        choices: &[],
    },
    Param {
        name: "body",
        kind: Kind::Str,
        required: false,
        doc: "The detail, under about 300 words.",
        choices: &[],
    },
    Param {
        name: "id",
        kind: Kind::Str,
        required: false,
        doc: "Slug, up to two segments. Made from the summary if absent.",
        choices: &[],
    },
    Param {
        name: "status",
        kind: Kind::Str,
        required: false,
        doc: "open or resolved.",
        choices: note::STATUSES,
    },
    Param {
        name: "evidence",
        kind: Kind::StrList,
        required: false,
        doc: "Pointers, each tool:reference.",
        choices: &[],
    },
    Param {
        name: "tags",
        kind: Kind::StrList,
        required: false,
        doc: "Tag slugs.",
        choices: &[],
    },
];

const RESOLVE_FIELDS: &[Param] = &[
    Param {
        name: "id",
        kind: Kind::Str,
        required: true,
        doc: "Note id.",
        choices: &[],
    },
    Param {
        name: "outcome",
        kind: Kind::Str,
        required: true,
        doc: "One line, up to 200 characters.",
        choices: &[],
    },
    Param {
        name: "supersede",
        kind: Kind::Bool,
        required: false,
        doc: "Mark the note replaced.",
        choices: &[],
    },
];

const ADD_FIELDS: &[Param] = &[
    Param {
        name: "title",
        kind: Kind::Str,
        required: true,
        doc: "One line, up to 200 characters.",
        choices: &[],
    },
    Param {
        name: "id",
        kind: Kind::Str,
        required: false,
        doc: "One slug. Made from the title if absent.",
        choices: &[],
    },
    Param {
        name: "parent",
        kind: Kind::Str,
        required: false,
        doc: "Parent task id. Omit for a new project.",
        choices: &[],
    },
    Param {
        name: "owner",
        kind: Kind::Str,
        required: false,
        doc: "Agent name that does it (sub-2). Default: you.",
        choices: &[],
    },
    Param {
        name: "status",
        kind: Kind::Str,
        required: false,
        doc: "Default todo.",
        choices: crate::board::STATUSES,
    },
];

const SET_FIELDS: &[Param] = &[
    Param { name: "id", kind: Kind::Str, required: true, doc: "Task id.", choices: &[] },
    Param { name: "status", kind: Kind::Str, required: false, doc: "New status.", choices: crate::board::STATUSES },
    Param { name: "title", kind: Kind::Str, required: false, doc: "New title.", choices: &[] },
    Param { name: "parent", kind: Kind::Str, required: false, doc: "Move under this task; empty string makes it a project.", choices: &[] },
    Param { name: "owner", kind: Kind::Str, required: false, doc: "Hand it to this agent.", choices: &[] },
    Param { name: "outcome", kind: Kind::Str, required: false, doc: "One line: what came of it. Empty string clears it.", choices: &[] },
    Param { name: "evidence", kind: Kind::StrList, required: false, doc: "Pointers, each tool:reference (adsertio:claim-7, trammel:plan-3/step-2). Replaces the list.", choices: &[] },
];

const OP_FIELDS: &[Param] = &[
    Param {
        name: "add",
        kind: Kind::Object(ADD_FIELDS),
        required: false,
        doc: "Add a task.",
        choices: &[],
    },
    Param {
        name: "set",
        kind: Kind::Object(SET_FIELDS),
        required: false,
        doc: "Change a task.",
        choices: &[],
    },
    Param {
        name: "remove",
        kind: Kind::Str,
        required: false,
        doc: "Delete a todo task with no children (a mistake). Use set status dropped otherwise.",
        choices: &[],
    },
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

fn archive_ledger(a: &Args) -> Result<Value, String> {
    crate::ledger_cmd::archive(a.str("agent").unwrap_or_default())
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

fn ledger_report(a: &Args) -> Result<Value, String> {
    crate::ledger_cmd::report(a)
}

fn check_tool(_a: &Args) -> Result<Value, String> {
    crate::check::run()
}
