//! `libellus check`: read every note and ledger and report damage.

use crate::fsx;
use crate::json::Value;
use crate::ledger;
use crate::note;
use crate::store::Store;

pub fn run() -> Result<Value, String> {
    let store = Store::open()?;
    let removed = crate::reap::reap_abandoned(&store.dir);
    let mut agents = Vec::new();
    let mut problems = Vec::new();
    let mut strays = Vec::new();
    for agent in store.agents()? {
        let loaded = note::load_all(&store, &agent)?;
        problems.extend(loaded.warnings);
        let ledger_read = ledger::read(&store, &agent)?;
        problems.extend(ledger_read.warnings.iter().cloned());
        if let Some(over) = ledger_read.sum().1 {
            let path = store.ledger_path(&agent)?;
            problems.push(ledger::overflow_text(&path, over.line));
        }
        let entries = ledger_read.entries;
        problems.extend(check_archives(&store, &agent)?);
        agents.push(Value::obj(vec![
            ("agent", Value::str(&agent)),
            ("notes", Value::uint(loaded.notes.len() as u64)),
            ("ledger_entries", Value::uint(entries.len() as u64)),
        ]));
        find_temps(&store.agent_dir(&agent)?, &mut strays);
    }
    // A lock that reap could not delete is still a problem.
    problems.extend(fsx::stale_lock_notes(&store.dir));
    if !problems.is_empty() {
        return Err(problems.join("; "));
    }
    Ok(Value::obj(vec![
        ("ok", Value::Bool(true)),
        ("agents", Value::Array(agents)),
        (
            "removed",
            Value::Array(
                removed
                    .iter()
                    .map(|s| Value::str(fsx::clip(s, 300)))
                    .collect(),
            ),
        ),
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

/// Archived ledgers are kept and are not part of the live cap. A damaged
/// archive is still an error. The file-size cap applies only to the live ledger.
fn check_archives(store: &Store, agent: &str) -> Result<Vec<String>, String> {
    let dir = store.archive_dir(agent)?;
    let rd = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    let mut problems = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let path = entry.path();
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.ends_with(".jsonl") {
            continue;
        }
        match ledger::read_at(&path, agent, false) {
            Ok(read) => {
                problems.extend(read.warnings.iter().cloned());
                if let Some(over) = read.sum().1 {
                    problems.push(ledger::overflow_text(&path, over.line));
                }
            }
            Err(err) => problems.push(err),
        }
    }
    Ok(problems)
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
