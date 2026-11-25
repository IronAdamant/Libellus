//! Spend and ledger commands. Split out of `tools` so that file stays small.

use crate::fsx;
use crate::json::Value;
use crate::ledger::{self, Entry};
use crate::note;
use crate::store::{Locked, Store};
use crate::tool::Args;

pub fn spend(a: &Args) -> Result<Value, String> {
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
    let recorded = ledger::append(&store, &entry)?;
    Ok(Value::obj(vec![
        ("recorded", Value::Bool(true)),
        ("task_total", recorded.task.to_value()),
        ("agent_total", recorded.agent.to_value()),
    ]))
}

/// Move the ledger aside into `archive/` and start a fresh one. The old bytes
/// are kept. Totals after this call are for the new file only.
pub fn archive(agent: &str) -> Result<Value, String> {
    let store = Store::open()?;
    let path = store.ledger_path(agent)?;
    if let Some(dir) = path.parent() {
        fsx::make_private_dir(dir)?;
    }
    let _lock = store.lock(Locked::Ledger(agent))?;
    fsx::refuse_special(&path)?;
    let len = match fsx::file_kind(&path)? {
        fsx::FileKind::Missing => {
            return Err(format!("no ledger to archive for agent \"{agent}\""));
        }
        fsx::FileKind::Regular { len: 0 } => {
            return Err(format!(
                "ledger {} is empty; nothing to archive",
                path.display()
            ));
        }
        fsx::FileKind::Regular { len } => len,
        fsx::FileKind::Symlink => {
            return Err(format!(
                "{} is a symlink; refusing to follow it",
                path.display()
            ));
        }
        fsx::FileKind::Other => {
            return Err(format!(
                "{} is not a regular file; refusing to open it",
                path.display()
            ));
        }
    };
    let archive_dir = store.archive_dir(agent)?;
    fsx::make_private_dir(&archive_dir)?;
    let stamp: String = fsx::now_iso()
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T' || *c == 'Z')
        .collect();
    let mut dest = archive_dir.join(format!("ledger-{stamp}.jsonl"));
    let mut n = 2u32;
    while matches!(
        fsx::file_kind(&dest)?,
        fsx::FileKind::Regular { .. } | fsx::FileKind::Symlink | fsx::FileKind::Other
    ) {
        dest = archive_dir.join(format!("ledger-{stamp}-{n}.jsonl"));
        n += 1;
        if n > 1000 {
            return Err("could not pick an archive name".into());
        }
    }
    std::fs::rename(&path, &dest).map_err(|e| {
        format!(
            "cannot archive {} to {}: {e}. The ledger was not changed.",
            path.display(),
            dest.display()
        )
    })?;
    let meta = path.with_file_name("ledger.meta");
    if matches!(fsx::file_kind(&meta)?, fsx::FileKind::Regular { .. }) {
        let meta_dest = dest.with_extension("meta");
        let _ = std::fs::rename(&meta, &meta_dest);
    }
    Ok(Value::obj(vec![
        ("agent", Value::str(agent)),
        ("archived", Value::String(dest.display().to_string())),
        ("bytes", Value::uint(len)),
    ]))
}

pub fn report(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let task = a.str("task");
    let step = a.str("step");
    if let Some(name) = task {
        if !fsx::valid_slug(name) {
            return Err(format!(
                "invalid task \"{}\": use a slug",
                fsx::clip(name, 40)
            ));
        }
    }
    if let Some(name) = step {
        if !fsx::valid_slug(name) {
            return Err(format!(
                "invalid step \"{}\": use a slug",
                fsx::clip(name, 40)
            ));
        }
    }
    let agents = match a.str("agent") {
        Some(name) => {
            store.agent_dir(name)?;
            vec![name.to_string()]
        }
        None => store.agents()?,
    };
    let mut entries = Vec::new();
    // Every problem is kept: a left-out line and an overflow can both apply.
    let mut problems: Vec<String> = Vec::new();
    for name in &agents {
        let summed = ledger::load_report(&store, name)?;
        let mut rows = summed.entries;
        if let Some(warning) = summed.warnings.into_iter().next() {
            problems.push(warning);
        }
        if let Some(line) = summed.overflow_line {
            let path = store.ledger_path(name)?;
            problems.push(format!(
                "{}. Totals stop before that line.",
                ledger::overflow_text(&path, line)
            ));
        }
        if let Some(task) = task {
            rows.retain(|e| e.task == task);
        }
        if let Some(step) = step {
            rows.retain(|e| e.step.as_deref() == Some(step));
        }
        entries.extend(rows);
    }
    let (totals, combined) = ledger::sum_prefix(&entries);
    if let Some(index) = combined {
        entries.truncate(index);
        problems.push(
            "ledger totals overflowed a 64-bit number while adding agents together. Totals stop before that point.".into(),
        );
    }
    let mut pairs = match totals.to_value() {
        Value::Object(fields) => fields,
        _ => Vec::new(),
    };
    pairs.insert(
        0,
        (
            "agents".to_string(),
            Value::Array(agents.iter().map(Value::str).collect()),
        ),
    );
    if task.is_none() {
        pairs.push((
            "by_task".to_string(),
            totals_map(&ledger::by_task(&entries)?),
        ));
    }
    if step.is_none() {
        pairs.push((
            "by_step".to_string(),
            totals_map(&ledger::by_step(&entries)?),
        ));
    }
    if !problems.is_empty() {
        pairs.push((
            "ledger_error".to_string(),
            Value::String(problems.join("; ")),
        ));
    }
    Ok(Value::Object(pairs))
}

fn totals_map(rows: &[(String, ledger::Totals)]) -> Value {
    Value::Object(
        rows.iter()
            .map(|(k, t)| (k.clone(), t.to_value()))
            .collect(),
    )
}
