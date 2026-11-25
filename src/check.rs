//! `libellus check`: read every note and ledger and report damage.

use crate::fsx;
use crate::json::Value;
use crate::ledger;
use crate::note;
use crate::store::Store;

pub fn run() -> Result<Value, String> {
    let store = Store::open()?;
    let mut agents = Vec::new();
    let mut problems = Vec::new();
    let mut strays = Vec::new();
    for agent in store.agents()? {
        let loaded = note::load_all(&store, &agent)?;
        problems.extend(loaded.warnings);
        let ledger_read = ledger::read(&store, &agent)?;
        problems.extend(ledger_read.warnings);
        let entries = ledger_read.entries;
        let (_totals, bad) = ledger::sum_prefix(&entries);
        if let Some(line) = bad {
            let path = store.ledger_path(&agent)?;
            problems.push(format!(
                "ledger {} line {line}: totals overflowed a 64-bit number",
                path.display()
            ));
        }
        agents.push(Value::obj(vec![
            ("agent", Value::str(&agent)),
            ("notes", Value::uint(loaded.notes.len() as u64)),
            ("ledger_entries", Value::uint(entries.len() as u64)),
        ]));
        find_temps(&store.agent_dir(&agent)?, &mut strays);
    }
    problems.extend(fsx::stale_lock_notes(&store.dir));
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    Ok(Value::obj(vec![
        ("ok", Value::Bool(true)),
        ("agents", Value::Array(agents)),
        (
            "stray_temp_files",
            Value::Array(
                strays
                    .iter()
                    .map(|s| Value::str(fsx::clip(s, 300)))
                    .collect(),
            ),
        ),
    ]))
}

fn find_temps(dir: &std::path::Path, out: &mut Vec<String>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                find_temps(&p, out);
            } else if p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.') && n.contains(".tmp-"))
            {
                out.push(p.display().to_string());
            }
        }
    }
}
