//! The tool table: what an agent can do, over MCP or the CLI.

use crate::fsx;
use crate::json::Value;
use crate::note::{self, Note};
use crate::promote;
use crate::store::Store;
use crate::tool::{Args, Kind, Param, Tool};

pub const GROUPS: &[&str] = &["core", "ledger"];

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
        description: "Write a private note before you forget a decision, a dead end, or an open question. Never overwrites: an existing id fails, and the error tells you to get it or pick a new id. Point at evidence as tool:reference instead of copying it.",
        params: &[
            AGENT,
            Param { name: "kind", kind: Kind::Str, required: true, doc: "What the note is.", choices: note::KINDS },
            Param { name: "summary", kind: Kind::Str, required: true, doc: "One line, up to 200 characters.", choices: &[] },
            Param { name: "body", kind: Kind::Str, required: false, doc: "The detail, under about 300 words, at most 20000 bytes.", choices: &[] },
            Param { name: "id", kind: Kind::Str, required: false, doc: "Slug, up to two segments (retry/post-question). Made from the summary if absent. Never overwrites.", choices: &[] },
            Param { name: "status", kind: Kind::Str, required: false, doc: "open or resolved. Default: open for a question, resolved otherwise.", choices: note::STATUSES },
            Param { name: "evidence", kind: Kind::StrList, required: false, doc: "Pointers to proof, each tool:reference (trammel:plan-3/step-2). No copies.", choices: &[] },
            Param { name: "tags", kind: Kind::StrList, required: false, doc: "Tag slugs.", choices: &[] },
        ],
        run: note_add,
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
        description: "Mark one of your notes resolved and append a one-line outcome. A note that is already resolved is left unchanged unless supersede is set, or the outcome starts with SUPERSEDED: or REVERSED:.",
        params: &[
            AGENT,
            NOTE_ID,
            Param {
                name: "outcome",
                kind: Kind::Str,
                required: true,
                doc: "One line, up to 200 characters, appended to the body. SUPERSEDED: or REVERSED: marks the note replaced.",
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
        run: note_resolve,
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
        description: "Call this when a session starts and after compaction, before other work. Returns open questions, the latest context, recent decisions, replaced decisions, dead ends, findings, costs, and a short text. max_items caps every section (default 5, at most 50). The text is capped at 8192 bytes and says when it was cut. A ledger that cannot be read sets ledger_error and still returns the notes. A Next line is added only when a question is open or a decision or dead end is not promoted.",
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
        description: "Read every note and ledger. Fails if a file is damaged, a note is over the size cap, a total overflows, a note's agent field does not match its folder, a ledger line was left out, or a stale lock is left behind. Stale locks and leftover temp files are removed first.",
        params: &[],
        run: check_tool,
    },
];

fn note_add(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    let kind = a.str("kind").unwrap_or_default();
    let summary = a.str("summary").unwrap_or_default();
    note::validate_summary(summary)?;
    let evidence: Vec<String> = a.list("evidence").iter().map(|s| s.to_string()).collect();
    if let Some(bad) = evidence.iter().find(|p| !note::valid_pointer(p)) {
        return Err(format!(
            "bad evidence pointer \"{}\": use tool:reference with no spaces, commas or brackets",
            fsx::clip(bad, 60)
        ));
    }
    let mut tags: Vec<String> = a.list("tags").iter().map(|s| s.to_string()).collect();
    if let Some(bad) = tags.iter().find(|t| !fsx::valid_slug(t)) {
        return Err(format!("bad tag \"{}\": use a slug", fsx::clip(bad, 40)));
    }
    let mut seen_tags = Vec::new();
    tags.retain(|tag| {
        if seen_tags.contains(tag) {
            false
        } else {
            seen_tags.push(tag.clone());
            true
        }
    });
    let notes_dir = store.notes_dir(agent)?;
    let seq = crate::seq::alloc_seq(&store, agent)?;
    let given = a.str("id").map(str::to_string);
    let attempts: u32 = if given.is_some() { 1 } else { 999 };
    for ntry in 1..=attempts {
        let id = match &given {
            Some(id) => id.clone(),
            None => candidate_id(summary, ntry),
        };
        let path = fsx::join_id(&notes_dir, &id, 2)?.with_extension("md");
        if let Some(dir) = path.parent() {
            fsx::make_private_dir(dir)?;
        }
        fsx::refuse_special(&path)?;
        let n = Note {
            id: id.clone(),
            kind: kind.to_string(),
            status: a
                .str("status")
                .unwrap_or(if kind == "question" {
                    "open"
                } else {
                    "resolved"
                })
                .to_string(),
            summary: summary.to_string(),
            agent: agent.to_string(),
            created: fsx::now_iso(),
            evidence: evidence.clone(),
            tags: tags.clone(),
            promoted: None,
            seq: Some(seq),
            superseded: false,
            body: a.str("body").unwrap_or_default().to_string(),
        };
        match fsx::write_atomic(&path, n.render().as_bytes(), false) {
            Ok(()) => {
                return Ok(Value::obj(vec![
                    ("id", Value::String(id)),
                    ("kind", Value::str(kind)),
                    ("status", Value::String(n.status)),
                ]));
            }
            Err(e) if given.is_none() && e.contains("already exists") => continue,
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

fn candidate_id(summary: &str, n: u32) -> String {
    let mut slug: String = summary
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    slug.truncate(48);
    let slug = slug.trim_matches('-').to_string();
    // A summary like "Con" must not become a Windows device name.
    let slug = if slug.is_empty() {
        "note".to_string()
    } else if fsx::reserved_name(&slug) {
        format!("{slug}-note")
    } else {
        slug
    };
    if n <= 1 {
        slug
    } else {
        format!("{slug}-{n}")
    }
}

fn note_get(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    let id = a.str("id").unwrap_or_default();
    Ok(note_full(&note::load_one(&store, agent, id)?))
}

fn note_resolve(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    let id = a.str("id").unwrap_or_default();
    let outcome = a.str("outcome").unwrap_or_default();
    note::validate_outcome(outcome)?;
    let path = note::note_path(&store, agent, id)?;
    let _lock = if path.parent().is_some_and(|parent| parent.is_dir()) {
        Some(fsx::Lock::acquire(&path)?)
    } else {
        None
    };
    let mut note = note::load_one(&store, agent, id)?;
    let supersede = a.bool("supersede").unwrap_or(false) || note::marks_replaced(outcome);
    if note.status == "resolved" && !supersede {
        return Err(format!(
            "note \"{id}\" is already resolved. The file was not changed. Read it with libellus_get. To mark it replaced, pass supersede or an outcome that starts with SUPERSEDED: or REVERSED:."
        ));
    }
    note.status = "resolved".to_string();
    if supersede {
        note.superseded = true;
    }
    let body = note::append_outcome(&note.body, outcome);
    note.body = body;
    let path = note::note_path(&store, agent, id)?;
    fsx::write_atomic(&path, note.render().as_bytes(), true)
        .map_err(|e| format!("note \"{id}\" not updated: {e}"))?;
    Ok(note_full(&note))
}

/// Every field of one note. The body is not cut: this is a single note, not a list.
fn note_full(n: &Note) -> Value {
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
