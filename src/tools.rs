//! The tool table: what an agent can do, over MCP or the CLI.

use crate::fsx;
use crate::json::Value;
use crate::ledger;
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
        description: "Mark one of your notes resolved and append a one-line outcome. A note that is already resolved is left unchanged and the error says so.",
        params: &[
            AGENT,
            NOTE_ID,
            Param {
                name: "outcome",
                kind: Kind::Str,
                required: true,
                doc: "One line, up to 200 characters, appended to the body.",
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
            Param { name: "offset", kind: Kind::UInt, required: false, doc: "Skip this many of the newest notes before applying limit. Default 0. Prefer before_id: a note written between pages can show up twice with offset.", choices: &[] },
            Param { name: "before_id", kind: Kind::Str, required: false, doc: "Stable cursor: only notes older than this id (by created, then id). Use next_before_id from the previous page.", choices: &[] },
            Param { name: "bodies", kind: Kind::Bool, required: false, doc: "Include each body (cut at 4000 characters).", choices: &[] },
        ],
        run: note_ls,
    },
    Tool {
        name: "libellus_brief",
        group: "core",
        description: "Call this when a session starts and after compaction, before other work. Returns open questions, recent decisions, dead ends, costs, and a short text you can paste back. A Next line is added only when a question is open or a decision or dead end is not promoted.",
        params: &[
            AGENT,
            Param { name: "max_items", kind: Kind::UInt, required: false, doc: "Most notes per section. Default 5.", choices: &[] },
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
        description: "Totals of tokens and cost. Pass agent, task, or step to narrow. Omit agent to sum every agent in the store. A damaged ledger is an error, names the file, and is not repaired.",
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
        name: "libellus_check",
        group: "core",
        description: "Read every note and ledger. Fails if a file is damaged, a note is over the size cap, a total overflows, a note's agent field does not match its folder, a ledger line was left out, or a stale lock is left behind.",
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
    let slug = if slug.is_empty() {
        "note".to_string()
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
    if note.status == "resolved" {
        return Err(format!(
            "note \"{id}\" is already resolved. The file was not changed. Read it with libellus_get."
        ));
    }
    note.status = "resolved".to_string();
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
    let mut notes = loaded.notes;
    notes.retain(|n| {
        a.str("kind").is_none_or(|k| n.kind == k) && a.str("status").is_none_or(|s| n.status == s)
    });
    let total = notes.len();
    let limit = a.uint("limit").unwrap_or(50) as usize;
    let offset = a.uint("offset").unwrap_or(0) as usize;
    if let Some(id) = a.str("before_id") {
        let cursor = notes.iter().find(|n| n.id == id).cloned().ok_or_else(|| {
            format!("before_id \"{id}\" is not in this list. Call libellus_ls without before_id.")
        })?;
        notes.retain(|n| {
            (n.created.as_str(), n.id.as_str()) < (cursor.created.as_str(), cursor.id.as_str())
        });
    }
    // Newest notes are at the end. offset skips that many, then limit keeps
    // the newest of what remains. offset 0 with no before_id is the 1.0.0 page.
    // before_id is the stable cursor: new notes written later are newer, so
    // they do not appear again on the next older page.
    if offset >= notes.len() {
        notes.clear();
    } else if offset > 0 {
        notes.truncate(notes.len() - offset);
    }
    let truncated = notes.len() > limit;
    if truncated {
        notes.drain(..notes.len() - limit);
    }
    let returned = notes.len() as u64;
    let next_before = if truncated {
        notes
            .first()
            .map(|n| Value::str(&n.id))
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let next_offset = if truncated {
        Value::uint(offset as u64 + returned)
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

fn brief(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    let max = a.uint("max_items").unwrap_or(5) as usize;
    let loaded = note::load_all(&store, agent)?;
    let notes = loaded.notes;
    let summed = ledger::read_summed(&store, agent)?;
    let entries = summed.entries;
    let totals = summed.totals;
    let overflow_line = summed.overflow_line;
    let ledger_warnings = summed.warnings;
    let tasks = ledger::by_task(&entries)?;
    let ledger_error = overflow_line.map(|line| {
        format!(
            "ledger totals overflowed a 64-bit number at line {line}. Totals above stop before that line."
        )
    });
    let latest = |kind: &str, status: Option<&str>| -> Vec<&Note> {
        let mut v: Vec<&Note> = notes
            .iter()
            .filter(|n| n.kind == kind && status.is_none_or(|s| n.status == s))
            .collect();
        if v.len() > max {
            v.drain(..v.len() - max);
        }
        v
    };
    let open: Vec<&Note> = notes
        .iter()
        .filter(|n| n.kind == "question" && n.status == "open")
        .collect();
    let decisions = latest("decision", None);
    let dead_ends = latest("dead-end", None);
    let mut text = format!(
        "# Libellus brief for {agent}\n\n## Open questions ({})\n",
        open.len()
    );
    for n in &open {
        text.push_str(&format!("- {}: {}\n", n.id, n.summary));
    }
    text.push_str("\n## Recent decisions\n");
    for n in &decisions {
        text.push_str(&format!("- {}: {}\n", n.id, n.summary));
    }
    text.push_str("\n## Dead ends\n");
    for n in &dead_ends {
        text.push_str(&format!("- {}: {}\n", n.id, n.summary));
    }
    text.push_str(&format!(
        "\n## Costs\n- total: {} in, {} out tokens, {} micro-USD ({} ledger entries)\n",
        totals.tokens_in,
        totals.tokens_out,
        totals.cost_micro_usd,
        entries.len()
    ));
    for (task, t) in tasks.iter().take(max) {
        text.push_str(&format!(
            "- {task}: {} in, {} out, {} micro-USD\n",
            t.tokens_in, t.tokens_out, t.cost_micro_usd
        ));
    }
    let mut next_bits = Vec::new();
    if !open.is_empty() {
        next_bits.push("resolve an open question with libellus_resolve");
    }
    let unshared = decisions
        .iter()
        .chain(dead_ends.iter())
        .any(|n| n.promoted.is_none());
    if unshared {
        next_bits.push("share a note with libellus_promote");
    }
    if !next_bits.is_empty() {
        text.push_str("\nNext: ");
        text.push_str(&next_bits.join("; "));
        text.push_str(".\n");
    }
    for warning in loaded.warnings.iter().chain(ledger_warnings.iter()) {
        text.push_str(&format!("\nWarning: {warning}\n"));
    }
    if let Some(err) = &ledger_error {
        text.push_str(&format!("\nLedger: {err}\n"));
    }
    let short = |v: &[&Note]| Value::Array(v.iter().map(|n| note_value(n, false)).collect());
    let mut pairs = vec![
        ("agent".to_string(), Value::str(agent)),
        ("open_questions".to_string(), short(&open)),
        ("recent_decisions".to_string(), short(&decisions)),
        ("dead_ends".to_string(), short(&dead_ends)),
        (
            "costs".to_string(),
            Value::obj(vec![
                ("total", totals.to_value()),
                (
                    "by_task",
                    Value::Object(
                        tasks
                            .iter()
                            .map(|(k, t)| (k.clone(), t.to_value()))
                            .collect(),
                    ),
                ),
            ]),
        ),
        ("text".to_string(), Value::String(text)),
    ];
    if let Some(err) = ledger_error {
        pairs.push(("ledger_error".to_string(), Value::String(err)));
    }
    let mut warnings = loaded.warnings;
    warnings.extend(ledger_warnings);
    if !warnings.is_empty() {
        pairs.push((
            "warnings".to_string(),
            Value::Array(warnings.iter().map(Value::str).collect()),
        ));
    }
    Ok(Value::Object(pairs))
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
