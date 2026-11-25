//! `libellus check`: read every note and ledger and report damage.

use crate::fsx;
use crate::json::Value;
use crate::ledger;
use crate::note;
use crate::store::Store;

/// What a read of the whole store found. Nothing is changed by the scan.
pub struct Scan {
    pub agents: Vec<Value>,
    pub problems: Vec<String>,
    pub strays: Vec<String>,
    pub board_tasks: usize,
    pub archived_projects: usize,
}

pub fn run() -> Result<Value, String> {
    let store = Store::open()?;
    let removed = crate::reap::reap_abandoned(&store.dir);
    let scan = scan(&store)?;
    if !scan.problems.is_empty() {
        return Err(scan.problems.join("; "));
    }
    Ok(Value::obj(vec![
        ("ok", Value::Bool(true)),
        ("agents", Value::Array(scan.agents)),
        (
            "board",
            Value::obj(vec![
                ("tasks", Value::uint(scan.board_tasks as u64)),
                (
                    "archived_projects",
                    Value::uint(scan.archived_projects as u64),
                ),
            ]),
        ),
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
                scan.strays
                    .iter()
                    .map(|s| Value::str(fsx::clip(s, 300)))
                    .collect(),
            ),
        ),
    ]))
}

/// Read every note, ledger and board file. Read-only: `check` reaps first,
/// `doctor` does not.
pub fn scan(store: &Store) -> Result<Scan, String> {
    let mut agents = Vec::new();
    let mut problems = Vec::new();
    let mut strays = Vec::new();
    for agent in store.agents()? {
        let loaded = match note::load_all(store, &agent) {
            Ok(l) => l,
            Err(e) => {
                problems.push(e);
                continue;
            }
        };
        problems.extend(loaded.warnings);
        let ledger_read = match ledger::read(store, &agent) {
            Ok(r) => r,
            Err(e) => {
                problems.push(e);
                continue;
            }
        };
        problems.extend(ledger_read.warnings.iter().cloned());
        if let Some(over) = ledger_read.sum().1 {
            let path = store.ledger_path(&agent)?;
            problems.push(ledger::overflow_text(&path, over.line));
        }
        let entries = ledger_read.entries;
        match check_archives(store, &agent) {
            Ok(p) => problems.extend(p),
            Err(e) => problems.push(e),
        }
        agents.push(Value::obj(vec![
            ("agent", Value::str(&agent)),
            ("notes", Value::uint(loaded.notes.len() as u64)),
            ("ledger_entries", Value::uint(entries.len() as u64)),
        ]));
        find_temps(&store.agent_dir(&agent)?, &mut strays);
    }
    let (board_tasks, archived_projects) = check_board(store, &mut problems);
    if let Ok(dir) = crate::board::board_dir(store) {
        find_temps(&dir, &mut strays);
    }
    Ok(Scan {
        agents,
        problems,
        strays,
        board_tasks,
        archived_projects,
    })
}

/// Damaged task files, orphans, a closed task with an open child, a closed
/// project that was not archived, a damaged counter or archive.
fn check_board(store: &Store, problems: &mut Vec<String>) -> (usize, usize) {
    use crate::board::{self, is_open};
    let live = match board::load(store) {
        Ok(b) => b,
        Err(e) => {
            problems.push(e);
            return (0, 0);
        }
    };
    problems.extend(live.warnings.iter().cloned());
    let mut ids: Vec<&String> = live.tasks.keys().collect();
    ids.sort();
    for id in ids {
        let t = &live.tasks[id];
        if is_open(&t.status) {
            continue;
        }
        let open: Vec<&str> = live
            .children(id)
            .iter()
            .filter(|c| live.get(c).is_some_and(|c| is_open(&c.status)))
            .map(String::as_str)
            .collect();
        if !open.is_empty() {
            problems.push(format!(
                "board task \"{id}\" is {} but has open children ({}). Set it back to todo or doing with libellus_plan, or close the children.",
                t.status,
                open.join(", ")
            ));
        }
        if t.parent.is_empty() {
            problems.push(format!(
                "board project \"{id}\" is closed but still on the live board (a write was cut short). The next libellus_plan call archives it."
            ));
        }
    }
    if let Ok(dir) = board::board_dir(store) {
        if let Err(e) = crate::seq::read_counter(&dir.join("seq")) {
            problems.push(e);
        }
    }
    let archived = match board::load_archive(store) {
        Ok(a) => a.len(),
        Err(e) => {
            problems.push(e);
            0
        }
    };
    (live.tasks.len(), archived)
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
