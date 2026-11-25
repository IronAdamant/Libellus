//! Monotonic per-agent note sequence. The counter file is `seq`, beside the
//! notes directory. A missing file starts from the highest `seq` already in
//! a note, or 0. A damaged counter is an error and is not reset.

use crate::fsx;
use crate::note;
use crate::store::Store;

pub fn alloc_seq(store: &Store, agent: &str) -> Result<u64, String> {
    let dir = store.agent_dir(agent)?;
    fsx::make_private_dir(&dir)?;
    let path = dir.join("seq");
    let _lock = fsx::Lock::acquire(&path)?;
    let current = match fsx::file_kind(&path)? {
        fsx::FileKind::Missing => max_seq(store, agent)?,
        fsx::FileKind::Symlink => {
            return Err(format!(
                "{} is a symlink; refusing to follow it",
                path.display()
            ))
        }
        fsx::FileKind::Other => {
            return Err(format!(
                "{} is not a regular file; refusing to open it",
                path.display()
            ))
        }
        fsx::FileKind::Regular { len } if len > 64 => {
            return Err(format!(
                "seq file {} is {len} bytes, which is not a counter",
                path.display()
            ))
        }
        fsx::FileKind::Regular { .. } => {
            let text = fsx::read_strict(&path)?;
            note::parse_seq(text.trim())
                .map_err(|e| format!("corrupt seq file {}: {e}", path.display()))?
        }
    };
    let next = current
        .checked_add(1)
        .ok_or_else(|| "note sequence overflowed a 64-bit number".to_string())?;
    fsx::write_atomic(&path, format!("{next}\n").as_bytes(), true)
        .map_err(|e| format!("seq file {} not written: {e}", path.display()))?;
    Ok(next)
}

fn max_seq(store: &Store, agent: &str) -> Result<u64, String> {
    let loaded = note::load_all(store, agent)?;
    Ok(loaded.notes.iter().filter_map(|n| n.seq).max().unwrap_or(0))
}
