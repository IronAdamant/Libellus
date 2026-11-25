//! The session brief: what an agent still needs after compaction.
//!
//! Sections are capped by `max_items` (at most 50) and the text is capped at
//! 8 KiB. Open questions, decisions, findings and task costs are newest
//! first, so a host that cuts the tail keeps the latest items. A ledger that
//! cannot be read does not blank the notes: costs are zero and `ledger_error`
//! says why.

use crate::json::Value;
use crate::ledger::{self, Totals};
use crate::note::{self, Note};
use crate::store::Store;
use crate::tool::Args;

const MAX_ITEMS: usize = 50;
const MAX_TEXT: usize = 8192;
const STATE_CHARS: usize = 500;

pub fn run(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    let _ = crate::reap::reap_abandoned(&store.dir);
    let requested = a.uint("max_items").unwrap_or(5) as usize;
    let max = if requested == 0 {
        0
    } else {
        requested.min(MAX_ITEMS)
    };
    let mut truncated = requested > MAX_ITEMS;
    let loaded = note::load_all(&store, agent)?;
    let notes = loaded.notes;
    let (totals, tasks, entry_count, ledger_error, ledger_warnings) =
        match ledger::load_report(&store, agent) {
            Ok(summed) => {
                let err = match (summed.overflow_line, store.ledger_path(agent)) {
                    (Some(line), Ok(path)) => Some(format!(
                        "{}. Totals above stop before that line.",
                        ledger::overflow_text(&path, line)
                    )),
                    _ => None,
                };
                let tasks = ledger::by_task_recent(&summed.entries)?;
                (
                    summed.totals,
                    tasks,
                    summed.entries.len(),
                    err.map(|text| cap_hint(text, agent)),
                    summed.warnings,
                )
            }
            Err(err) => (
                Totals::default(),
                Vec::new(),
                0,
                Some(cap_hint(err, agent)),
                Vec::new(),
            ),
        };

    let (open, open_total) = newest(&notes, |n| n.kind == "question" && n.status == "open", max);
    let (decisions, decision_total) =
        newest(&notes, |n| n.kind == "decision" && !n.superseded, max);
    let (replaced, replaced_total) = newest(&notes, |n| n.kind == "decision" && n.superseded, max);
    let (dead_ends, dead_total) = newest(&notes, |n| n.kind == "dead-end", max);
    let (findings, finding_total) = newest(&notes, |n| n.kind == "finding", max);
    let context = notes.iter().rev().find(|n| n.kind == "context");
    if open_total > open.len()
        || decision_total > decisions.len()
        || replaced_total > replaced.len()
        || dead_total > dead_ends.len()
        || finding_total > findings.len()
        || tasks.len() > max
    {
        truncated = true;
    }
    let shown_tasks = tasks.len().min(max);

    let mut text = format!("# Libellus brief for {agent}\n\n## Current state\n");
    match context {
        Some(note) => {
            text.push_str(&format!("- {}: {}\n", note.id, note.summary));
            if !note.body.is_empty() {
                let (body, cut) = clip_chars(&note.body, STATE_CHARS);
                text.push_str(&format!("  {body}\n"));
                if cut {
                    text.push_str(&format!(
                        "  Body cut at {STATE_CHARS} characters. Read it with libellus_get.\n"
                    ));
                }
            }
        }
        None => {
            text.push_str("- none. Write the latest state with libellus_note --kind context.\n");
        }
    }
    text.push('\n');
    if open_total > open.len() {
        text.push_str(&format!(
            "## Open questions ({open_total}, showing {})\n",
            open.len()
        ));
    } else {
        text.push_str(&format!("## Open questions ({open_total})\n"));
    }
    for note in &open {
        text.push_str(&format!("- {}: {}\n", note.id, note.summary));
    }
    if open_total > open.len() {
        text.push_str(&format!(
            "Showing the newest {} of {open_total}. {} older questions omitted.\n",
            open.len(),
            open_total - open.len()
        ));
    }
    text.push_str("\n## Recent decisions\n");
    for note in &decisions {
        text.push_str(&format!("- {}: {}\n", note.id, note.summary));
    }
    if decision_total > decisions.len() {
        text.push_str(&format!(
            "Showing the newest {} of {decision_total} current decisions.\n",
            decisions.len()
        ));
    }
    if replaced_total > 0 {
        text.push_str("\n## Replaced decisions\n");
        for note in &replaced {
            text.push_str(&format!("- {}: {} (replaced)\n", note.id, note.summary));
        }
        if replaced_total > replaced.len() {
            text.push_str(&format!(
                "Showing the newest {} of {replaced_total} replaced decisions.\n",
                replaced.len()
            ));
        }
    }
    text.push_str("\n## Dead ends\n");
    for note in &dead_ends {
        text.push_str(&format!("- {}: {}\n", note.id, note.summary));
    }
    if !findings.is_empty() {
        text.push_str("\n## Recent findings\n");
        for note in &findings {
            text.push_str(&format!("- {}: {}\n", note.id, note.summary));
        }
        if finding_total > findings.len() {
            text.push_str(&format!(
                "Showing the newest {} of {finding_total} findings.\n",
                findings.len()
            ));
        }
    }
    let entries = if entry_count == 1 { "entry" } else { "entries" };
    text.push_str(&format!(
        "\n## Costs\n- total: {} in, {} out tokens, {} micro-USD ({entry_count} ledger {entries})\n",
        totals.tokens_in, totals.tokens_out, totals.cost_micro_usd
    ));
    for (task, row) in tasks.iter().take(max) {
        text.push_str(&format!(
            "- {task}: {} in, {} out, {} micro-USD\n",
            row.tokens_in, row.tokens_out, row.cost_micro_usd
        ));
    }
    if tasks.len() > shown_tasks {
        text.push_str(&format!(
            "Showing {shown_tasks} of {} tasks, newest spend first. The rest are in libellus_ledger.\n",
            tasks.len()
        ));
    }
    for warning in loaded.warnings.iter().chain(ledger_warnings.iter()) {
        text.push_str(&format!("\nWarning: {warning}\n"));
    }
    if let Some(err) = &ledger_error {
        text.push_str(&format!("\nLedger: {err}\n"));
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
    let next = if next_bits.is_empty() {
        String::new()
    } else {
        format!("\nNext: {}.\n", next_bits.join("; "))
    };
    let notice = format!(
        "\nBrief truncated at {MAX_TEXT} bytes. Call libellus_ls or libellus_get for the rest.\n"
    );
    if text.len() + next.len() > MAX_TEXT {
        let room = MAX_TEXT.saturating_sub(next.len() + notice.len());
        truncate_text(&mut text, room);
        text.push_str(&notice);
        truncated = true;
    }
    text.push_str(&next);

    let task_rows: Vec<(String, Value)> = tasks
        .iter()
        .take(max)
        .map(|(key, row)| (key.clone(), row.to_value()))
        .collect();
    let mut pairs = vec![
        ("agent".to_string(), Value::str(agent)),
        ("open_questions".to_string(), notes_json(&open)),
        (
            "open_questions_total".to_string(),
            Value::uint(open_total as u64),
        ),
        ("recent_decisions".to_string(), notes_json(&decisions)),
        ("replaced_decisions".to_string(), notes_json(&replaced)),
        ("dead_ends".to_string(), notes_json(&dead_ends)),
        ("recent_findings".to_string(), notes_json(&findings)),
        ("context".to_string(), context_json(context)),
        (
            "costs".to_string(),
            Value::obj(vec![
                ("total", totals.to_value()),
                ("by_task", Value::Object(task_rows)),
                ("by_task_total", Value::uint(tasks.len() as u64)),
                (
                    "by_task_omitted",
                    Value::uint(tasks.len().saturating_sub(shown_tasks) as u64),
                ),
            ]),
        ),
        ("truncated".to_string(), Value::Bool(truncated)),
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

fn cap_hint(mut err: String, agent: &str) -> String {
    if err.contains("byte cap") && !err.contains("line cap") && !err.contains("libellus archive") {
        err.push_str(&format!(
            " Archive it with libellus archive --agent {agent}. The old lines are kept. Notes in this brief are still listed."
        ));
    }
    err
}

fn newest(notes: &[Note], pred: impl Fn(&Note) -> bool, max: usize) -> (Vec<&Note>, usize) {
    let all: Vec<&Note> = notes.iter().filter(|n| pred(n)).collect();
    let total = all.len();
    let start = total.saturating_sub(max);
    let mut page: Vec<&Note> = all[start..].to_vec();
    page.reverse();
    (page, total)
}

fn notes_json(notes: &[&Note]) -> Value {
    Value::Array(notes.iter().copied().map(note_json).collect())
}

fn note_json(note: &Note) -> Value {
    let mut pairs = vec![
        ("id", Value::str(&note.id)),
        ("kind", Value::str(&note.kind)),
        ("status", Value::str(&note.status)),
        ("summary", Value::str(&note.summary)),
        ("created", Value::str(&note.created)),
    ];
    if let Some(seq) = note.seq {
        pairs.push(("seq", Value::uint(seq)));
    }
    if note.superseded {
        pairs.push(("superseded", Value::Bool(true)));
    }
    if !note.evidence.is_empty() {
        pairs.push((
            "evidence",
            Value::Array(note.evidence.iter().map(Value::str).collect()),
        ));
    }
    if let Some(promoted) = &note.promoted {
        pairs.push(("promoted", Value::str(promoted)));
    }
    Value::obj(pairs)
}

fn context_json(note: Option<&Note>) -> Value {
    let Some(note) = note else {
        return Value::Null;
    };
    let mut value = note_json(note);
    let (body, cut) = clip_chars(&note.body, STATE_CHARS);
    if let Value::Object(pairs) = &mut value {
        pairs.push(("body".to_string(), Value::String(body)));
        if cut {
            pairs.push(("truncated".to_string(), Value::Bool(true)));
        }
    }
    value
}

fn clip_chars(text: &str, max_chars: usize) -> (String, bool) {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let count = flat.chars().count();
    if count > max_chars {
        let clipped: String = flat.chars().take(max_chars).collect();
        (format!("{clipped}..."), true)
    } else {
        (flat, false)
    }
}

fn truncate_text(text: &mut String, max_bytes: usize) {
    if text.len() <= max_bytes {
        return;
    }
    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}
