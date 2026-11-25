//! Spend and ledger commands. Split out of `tools` so that file stays small.

use crate::fsx;
use crate::json::Value;
use crate::ledger::{self, Entry};
use crate::note;
use crate::store::Store;
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
    let all = ledger::append(&store, &entry)?;
    let mine: Vec<Entry> = all.iter().filter(|e| e.task == task).cloned().collect();
    Ok(Value::obj(vec![
        ("recorded", Value::Bool(true)),
        ("task_total", ledger::total(&mine)?.to_value()),
        ("agent_total", ledger::total(&all)?.to_value()),
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
    let mut ledger_error = None;
    for name in &agents {
        let (mut rows, _totals, bad) = ledger::read_summed(&store, name)?;
        if let Some(line) = bad {
            let path = store.ledger_path(name)?;
            ledger_error = Some(format!(
                "ledger {} line {line}: totals overflowed a 64-bit number. Totals stop before that line.",
                path.display()
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
    if let Some(line) = combined {
        entries.truncate(line - 1);
        if ledger_error.is_none() {
            ledger_error = Some(
                "ledger totals overflowed a 64-bit number while adding agents together. Totals stop before that point.".into(),
            );
        }
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
    if let Some(err) = ledger_error {
        pairs.push(("ledger_error".to_string(), Value::String(err)));
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
