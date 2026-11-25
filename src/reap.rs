//! Remove stale locks and leftover temp files. A temp whose pid is still
//! running is left alone.

use crate::fsx;
use std::fs;
use std::path::Path;

/// Delete stale locks and abandoned temp files under `root`. Returns the
/// paths that were removed.
pub fn reap_abandoned(root: &Path) -> Vec<String> {
    let mut removed = Vec::new();
    reap_dir(root, &mut removed, true);
    removed
}

/// The stale locks and abandoned temp files `reap_abandoned` would remove.
/// Nothing is changed.
pub fn find_abandoned(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    reap_dir(root, &mut found, false);
    found
}

fn reap_dir(dir: &Path, removed: &mut Vec<String>, remove: bool) {
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
        if kind.is_dir() {
            reap_dir(&path, removed, remove);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let gone = if name.starts_with('.') && name.ends_with(".lock") {
            if remove {
                fsx::break_stale_lock(&path)
            } else {
                fsx::lock_is_stale(&path)
            }
        } else {
            name.starts_with('.')
                && name.contains(".tmp-")
                && temp_abandoned(&path)
                && (!remove || fs::remove_file(&path).is_ok())
        };
        if gone {
            removed.push(path.display().to_string());
        }
    }
}

fn temp_abandoned(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let Some(rest) = name.split(".tmp-").nth(1) else {
        return aged(path, fsx::LOCK_STALE);
    };
    let Some(pid) = rest.split('-').next().and_then(|s| s.parse().ok()) else {
        return aged(path, fsx::LOCK_STALE);
    };
    match fsx::pid_alive(Some(pid)) {
        Some(true) => false,
        Some(false) => true,
        None => aged(path, fsx::LOCK_STALE),
    }
}

fn aged(path: &Path, limit: std::time::Duration) -> bool {
    fs::symlink_metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > limit)
}
