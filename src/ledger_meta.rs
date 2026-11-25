//! Cache of ledger totals beside `ledger.jsonl`.
//!
//! The ledger file is the record. `ledger.meta` only lets `spend` skip a
//! parse when the file length and the last line still match. A hand edit
//! that keeps both is not seen until `brief`, `ledger` or `check` reads the
//! file and rewrites this cache. A damaged cache is ignored, not an error.

use crate::fsx;
use crate::json::{self, Value};
use crate::ledger::{self, Entry, Totals};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Per-task rows kept in the cache. Past this, the map is larger than the
/// win, and spend totals that one task from the ledger instead.
pub const MAX_CACHED_TASKS: usize = 64;

#[derive(Clone)]
pub struct Cache {
    pub len: u64,
    pub totals: Totals,
    pub tasks: HashMap<String, Totals>,
    /// False when `tasks` was dropped because it would have been too big.
    /// Agent totals are still valid.
    pub tasks_complete: bool,
    pub last_hash: u64,
}

pub fn hash_bytes(data: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn meta_path(ledger: &Path) -> std::path::PathBuf {
    ledger.with_file_name("ledger.meta")
}

/// The cache when it still describes this file, otherwise `None`.
pub fn matching(ledger: &Path, len: u64) -> Result<Option<Cache>, String> {
    if len == 0 {
        return Ok(Some(Cache {
            len: 0,
            totals: Totals::default(),
            tasks: HashMap::new(),
            tasks_complete: true,
            last_hash: 0,
        }));
    }
    let Some(cache) = load(&meta_path(ledger)) else {
        return Ok(None);
    };
    if cache.len != len {
        return Ok(None);
    }
    match tail_hash(ledger, len)? {
        Some(hash) if hash == cache.last_hash => Ok(Some(cache)),
        _ => Ok(None),
    }
}

/// Rebuild the cache from entries that were just read. No-op when the file
/// is missing, torn, or not a regular file.
pub fn store_entries(ledger: &Path, entries: &[Entry]) -> Result<(), String> {
    let len = match fsx::file_kind(ledger)? {
        fsx::FileKind::Regular { len } => len,
        _ => return Ok(()),
    };
    let Some(last_hash) = tail_hash(ledger, len)? else {
        return Ok(());
    };
    let mut totals = Totals::default();
    let mut tasks: HashMap<String, Totals> = HashMap::new();
    for entry in entries {
        totals.add(entry)?;
        tasks.entry(entry.task.clone()).or_default().add(entry)?;
    }
    let tasks_complete = tasks.len() <= MAX_CACHED_TASKS;
    if !tasks_complete {
        tasks.clear();
    }
    save(
        ledger,
        &Cache {
            len,
            totals,
            tasks,
            tasks_complete,
            last_hash,
        },
    )
}

pub fn save(ledger: &Path, cache: &Cache) -> Result<(), String> {
    let path = meta_path(ledger);
    if matches!(fsx::file_kind(&path)?, fsx::FileKind::Symlink) {
        return Ok(());
    }
    let mut tasks: Vec<&String> = cache.tasks.keys().collect();
    tasks.sort();
    let task_pairs = tasks
        .into_iter()
        .filter_map(|key| {
            cache
                .tasks
                .get(key)
                .map(|totals| (key.clone(), totals_value(totals)))
        })
        .collect();
    let body = Value::obj(vec![
        ("len", Value::uint(cache.len)),
        ("last", Value::uint(cache.last_hash)),
        ("tasks_complete", Value::Bool(cache.tasks_complete)),
        ("entries", Value::uint(cache.totals.entries)),
        ("tokens_in", Value::uint(cache.totals.tokens_in)),
        ("tokens_out", Value::uint(cache.totals.tokens_out)),
        ("cost_micro_usd", Value::uint(cache.totals.cost_micro_usd)),
        ("tasks", Value::Object(task_pairs)),
    ]);
    fsx::write_atomic(&path, body.to_json().as_bytes(), true)
}

fn totals_value(totals: &Totals) -> Value {
    Value::obj(vec![
        ("entries", Value::uint(totals.entries)),
        ("tokens_in", Value::uint(totals.tokens_in)),
        ("tokens_out", Value::uint(totals.tokens_out)),
        ("cost_micro_usd", Value::uint(totals.cost_micro_usd)),
    ])
}

fn load(path: &Path) -> Option<Cache> {
    // A FIFO, device or symlink is not a cache. Reading a FIFO would block
    // while the ledger lock is held.
    if !matches!(fsx::file_kind(path), Ok(fsx::FileKind::Regular { .. })) {
        return None;
    }
    let text = fsx::read_strict(path).ok()?;
    let value = json::parse(&text).ok()?;
    let totals = totals_of(&value)?;
    let len = value.get("len")?.as_u64()?;
    let last_hash = value.get("last")?.as_u64()?;
    let tasks_complete = value
        .get("tasks_complete")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut tasks = HashMap::new();
    if let Some(Value::Object(pairs)) = value.get("tasks") {
        for (key, row) in pairs {
            tasks.insert(key.clone(), totals_of(row)?);
        }
    } else {
        return None;
    }
    Some(Cache {
        len,
        totals,
        tasks,
        tasks_complete,
        last_hash,
    })
}

fn totals_of(value: &Value) -> Option<Totals> {
    Some(Totals {
        entries: value.get("entries")?.as_u64()?,
        tokens_in: value.get("tokens_in")?.as_u64()?,
        tokens_out: value.get("tokens_out")?.as_u64()?,
        cost_micro_usd: value.get("cost_micro_usd")?.as_u64()?,
    })
}

/// Hash of the last complete line, including its newline. `None` when the
/// file does not end on a line boundary.
pub fn tail_hash(path: &Path, len: u64) -> Result<Option<u64>, String> {
    if len == 0 {
        return Ok(Some(0));
    }
    let window = (ledger::MAX_LEDGER_LINE as u64 + 2).min(len);
    let mut file = File::open(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    file.seek(SeekFrom::End(-(window as i64)))
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut buf = vec![0u8; window as usize];
    file.read_exact(&mut buf)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if buf.last() != Some(&b'\n') {
        return Ok(None);
    }
    let end = buf.len() - 1;
    let start = buf[..end]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    Ok(Some(hash_bytes(&buf[start..])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_hash_is_the_last_complete_line() {
        let path = std::env::temp_dir().join(format!("libellus-tail-{}", std::process::id()));
        std::fs::write(&path, b"one\ntwo\n").unwrap();
        let len = std::fs::metadata(&path).unwrap().len();
        assert_eq!(tail_hash(&path, len).unwrap(), Some(hash_bytes(b"two\n")));
        std::fs::write(&path, b"one\ntwo").unwrap();
        let len = std::fs::metadata(&path).unwrap().len();
        assert_eq!(tail_hash(&path, len).unwrap(), None);
        let _ = std::fs::remove_file(&path);
    }
}
