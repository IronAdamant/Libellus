//! Monotonic counters. The note counter is `agents/<agent>/seq`, beside the
//! notes directory; the board counter is `board/seq`. A missing file starts
//! from the highest `seq` already stored, or 0. A damaged counter is an error
//! and is not reset.

use crate::fsx;
use crate::note;
use crate::store::Store;
use std::path::Path;

/// Take `n` note numbers for `agent` and return the first. The numbers are
/// `first..first + n`. A batch of notes takes its numbers in one locked step.
pub fn alloc_seqs(store: &Store, agent: &str, n: u64) -> Result<u64, String> {
    let dir = store.agent_dir(agent)?;
    fsx::make_private_dir(&dir)?;
    let path = dir.join("seq");
    let _lock = fsx::Lock::acquire(&path)?;
    let current = match read_counter(&path)? {
        Some(v) => v,
        None => max_seq(store, agent)?,
    };
    let first = current
        .checked_add(1)
        .ok_or_else(|| "note sequence overflowed a 64-bit number".to_string())?;
    let last = current
        .checked_add(n.max(1))
        .ok_or_else(|| "note sequence overflowed a 64-bit number".to_string())?;
    write_counter(&path, last)?;
    Ok(first)
}

/// The value in a counter file, or `None` when the file does not exist. The
/// caller holds the lock that guards it.
pub fn read_counter(path: &Path) -> Result<Option<u64>, String> {
    match fsx::file_kind(path)? {
        fsx::FileKind::Missing => Ok(None),
        fsx::FileKind::Symlink => Err(format!(
            "{} is a symlink; refusing to follow it",
            path.display()
        )),
        fsx::FileKind::Other => Err(format!(
            "{} is not a regular file; refusing to open it",
            path.display()
        )),
        fsx::FileKind::Regular { len } if len > 64 => Err(format!(
            "seq file {} is {len} bytes, which is not a counter",
            path.display()
        )),
        fsx::FileKind::Regular { .. } => {
            let text = fsx::read_strict(path)?;
            note::parse_seq(text.trim())
                .map(Some)
                .map_err(|e| format!("corrupt seq file {}: {e}", path.display()))
        }
    }
}

pub fn write_counter(path: &Path, value: u64) -> Result<(), String> {
    fsx::write_atomic(path, format!("{value}\n").as_bytes(), true)
        .map_err(|e| format!("seq file {} not written: {e}", path.display()))
}

fn max_seq(store: &Store, agent: &str) -> Result<u64, String> {
    let loaded = note::load_all(store, agent)?;
    Ok(loaded.notes.iter().filter_map(|n| n.seq).max().unwrap_or(0))
}
