//! The tool table: what an agent can do, over MCP or the CLI.

use crate::fsx;
use crate::json::Value;
use crate::ledger::{self, Entry};
use crate::note::{self, Note};
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

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "libellus_note",
        group: "core",
        description: "Write a private note: a decision, dead-end, question, finding or context. Questions start open; resolve them when answered. Point at evidence (tool:ref) instead of copying it.",
        params: &[
            AGENT,
            Param { name: "kind", kind: Kind::Str, required: true, doc: "What the note is.", choices: note::KINDS },
            Param { name: "summary", kind: Kind::Str, required: true, doc: "One line, up to 200 characters.", choices: &[] },
            Param { name: "body", kind: Kind::Str, required: false, doc: "The detail, under about 300 words.", choices: &[] },
            Param { name: "id", kind: Kind::Str, required: false, doc: "Slug, up to two segments (retry/post-question). Made from the summary if absent. Never overwrites.", choices: &[] },
            Param { name: "status", kind: Kind::Str, required: false, doc: "open or resolved. Default: open for a question, resolved otherwise.", choices: note::STATUSES },
            Param { name: "evidence", kind: Kind::StrList, required: false, doc: "Pointers to proof, each tool:reference (trammel:plan-3/step-2). No copies.", choices: &[] },
            Param { name: "tags", kind: Kind::StrList, required: false, doc: "Tag slugs.", choices: &[] },
        ],
        run: note_add,
    },
    Tool {
        name: "libellus_ls",
        group: "core",
        description: "List your own notes, oldest first, with optional filters. Set bodies to read the detail too.",
        params: &[
            AGENT,
            Param { name: "kind", kind: Kind::Str, required: false, doc: "Only this kind.", choices: note::KINDS },
            Param { name: "status", kind: Kind::Str, required: false, doc: "Only this status.", choices: note::STATUSES },
            Param { name: "limit", kind: Kind::UInt, required: false, doc: "Most notes to return, newest kept. Default 50.", choices: &[] },
            Param { name: "bodies", kind: Kind::Bool, required: false, doc: "Include each body (cut at 4000 characters).", choices: &[] },
        ],
        run: note_ls,
    },
    Tool {
        name: "libellus_brief",
        group: "core",
        description: "The compaction payload: your open questions, recent decisions and dead ends, and running costs. Call it when the context is squashed or a session resumes.",
        params: &[
            AGENT,
            Param { name: "max_items", kind: Kind::UInt, required: false, doc: "Most notes per section. Default 5.", choices: &[] },
        ],
        run: brief,
    },
    Tool {
        name: "libellus_spend",
        group: "ledger",
        description: "Record tokens and cost for a step. You report the numbers; Libellus only adds them up.",
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
    let tags: Vec<String> = a.list("tags").iter().map(|s| s.to_string()).collect();
    if let Some(bad) = tags.iter().find(|t| !fsx::valid_slug(t)) {
        return Err(format!("bad tag \"{}\": use a slug", fsx::clip(bad, 40)));
    }
    let notes_dir = store.notes_dir(agent)?;
    let id = match a.str("id") {
        Some(id) => id.to_string(),
        None => unique_id(&notes_dir, summary)?,
    };
    let path = fsx::join_id(&notes_dir, &id, 2)?.with_extension("md");
    if let Some(dir) = path.parent() {
        fsx::make_private_dir(dir)?;
    }
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
        evidence,
        tags,
        promoted: None,
        body: a.str("body").unwrap_or_default().to_string(),
    };
    fsx::write_atomic(&path, n.render().as_bytes(), false)
        .map_err(|e| format!("note \"{id}\" not written: {e}"))?;
    Ok(Value::obj(vec![
        ("id", Value::String(id)),
        ("kind", Value::str(kind)),
        ("status", Value::String(n.status)),
    ]))
}

fn unique_id(dir: &std::path::Path, summary: &str) -> Result<String, String> {
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
    for n in 1..1000 {
        let id = if n == 1 {
            slug.clone()
        } else {
            format!("{slug}-{n}")
        };
        if !dir.join(format!("{id}.md")).exists() {
            return Ok(id);
        }
    }
    Err("could not find a free id; pass id".into())
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
    let mut notes = note::load_all(&store, a.str("agent").unwrap_or_default())?;
    notes.retain(|n| {
        a.str("kind").is_none_or(|k| n.kind == k) && a.str("status").is_none_or(|s| n.status == s)
    });
    let total = notes.len();
    let limit = a.uint("limit").unwrap_or(50) as usize;
    if notes.len() > limit {
        notes.drain(..notes.len() - limit);
    }
    let bodies = a.bool("bodies").unwrap_or(false);
    Ok(Value::obj(vec![
        (
            "notes",
            Value::Array(notes.iter().map(|n| note_value(n, bodies)).collect()),
        ),
        ("total", Value::uint(total as u64)),
    ]))
}

fn brief(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    let max = a.uint("max_items").unwrap_or(5) as usize;
    let notes = note::load_all(&store, agent)?;
    let entries = ledger::read(&store, agent)?;
    let totals = ledger::total(&entries)?;
    let tasks = ledger::by_task(&entries)?;
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
        totals.tokens_in, totals.tokens_out, totals.cost_micro_usd, totals.entries
    ));
    for (task, t) in tasks.iter().take(max) {
        text.push_str(&format!(
            "- {task}: {} in, {} out, {} micro-USD\n",
            t.tokens_in, t.tokens_out, t.cost_micro_usd
        ));
    }
    let short = |v: &[&Note]| Value::Array(v.iter().map(|n| note_value(n, false)).collect());
    Ok(Value::obj(vec![
        ("agent", Value::str(agent)),
        ("open_questions", short(&open)),
        ("recent_decisions", short(&decisions)),
        ("dead_ends", short(&dead_ends)),
        (
            "costs",
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
        ("text", Value::String(text)),
    ]))
}

fn spend(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default().to_string();
    let task = a.str("task").unwrap_or_default();
    if !fsx::valid_slug(task) {
        return Err(format!(
            "invalid task \"{}\": use a slug",
            fsx::clip(task, 40)
        ));
    }
    let step = a.str("step");
    if let Some(s) = step {
        if !fsx::valid_slug(s) {
            return Err(format!("invalid step \"{}\": use a slug", fsx::clip(s, 40)));
        }
    }
    if let Some(n) = a.str("note") {
        note::validate_summary(n).map_err(|e| format!("note: {e}"))?;
    }
    let (ti, to, c) = (
        a.uint("tokens_in"),
        a.uint("tokens_out"),
        a.uint("cost_micro_usd"),
    );
    if ti.is_none() && to.is_none() && c.is_none() {
        return Err("give at least one of tokens_in, tokens_out, cost_micro_usd".into());
    }
    let entry = Entry {
        ts: fsx::now_iso(),
        agent: agent.clone(),
        task: task.to_string(),
        step: step.map(String::from),
        model: a.str("model").map(String::from),
        tokens_in: ti.unwrap_or(0),
        tokens_out: to.unwrap_or(0),
        cost_micro_usd: c.unwrap_or(0),
        note: a.str("note").map(String::from),
    };
    ledger::append(&store, &entry)?;
    let all = ledger::read(&store, &agent)?;
    let mine: Vec<Entry> = all.iter().filter(|e| e.task == task).cloned().collect();
    Ok(Value::obj(vec![
        ("recorded", Value::Bool(true)),
        ("task_total", ledger::total(&mine)?.to_value()),
        ("agent_total", ledger::total(&all)?.to_value()),
    ]))
}
