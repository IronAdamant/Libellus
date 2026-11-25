//! Remove leftovers: temp files older than `TEMP_STALE`, and the lock files
//! Libellus 1.3 left beside what they guarded (`.<name>.lock` and its
//! `.break` guard). The 1.4 lock files under `locks/` are operating-system
//! locks that are never stale and never deleted, so this leaves them alone.

use std::fs;
use std::path::Path;
use std::time::Duration;

/// How old a temp file must be before it is taken as abandoned. A write holds
/// its temp for one write, sync and rename: milliseconds. A writer paused
/// past this (a laptop asleep mid-write) loses only that write: its rename or
/// link then fails with an error, and no other file is touched, because each
/// temp name holds the writer's pid and a counter and is never reused.
pub const TEMP_STALE: Duration = Duration::from_secs(60 * 60);

/// Delete abandoned temp files and 1.3 lock files under `root`. Returns the
/// paths that were removed.
pub fn reap_abandoned(root: &Path) -> Vec<String> {
    let mut removed = Vec::new();
    reap_dir(root, &mut removed, true, true);
    removed
}

/// The files `reap_abandoned` would remove. Nothing is changed.
pub fn find_abandoned(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    reap_dir(root, &mut found, false, true);
    found
}

/// A lock file name from Libellus 1.3: `.<name>.lock` or `.<name>.lock.break`.
/// Nothing in 1.4 uses these names, so one is always a leftover.
pub fn old_lock_name(name: &str) -> bool {
    name.starts_with('.') && (name.ends_with(".lock") || name.ends_with(".lock.break"))
}

fn reap_dir(dir: &Path, removed: &mut Vec<String>, remove: bool, top: bool) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if kind.is_dir() {
            // The 1.4 lock files: held or not, they stay.
            if !(top && name == "locks") {
                reap_dir(&path, removed, remove, false);
            }
            continue;
        }
        let leftover = old_lock_name(name)
            || (name.starts_with('.') && name.contains(".tmp-") && aged(&path, TEMP_STALE));
        if leftover && (!remove || fs::remove_file(&path).is_ok()) {
            removed.push(path.display().to_string());
        }
    }
}

fn aged(path: &Path, limit: Duration) -> bool {
    fs::symlink_metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > limit)
}
