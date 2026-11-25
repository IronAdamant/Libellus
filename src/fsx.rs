//! File helpers: atomic writes with cleanup, strict reads, path guards.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Removes its file on drop unless disarmed, so a failed or panicking write
/// never leaves a temp file behind.
struct TempGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Write `bytes` to `dest` whole: a temp file beside it, synced, then either
/// renamed over `dest` (`overwrite`) or hard-linked into place so an existing
/// file is never replaced. A reader sees the old file, the new file, or no
/// file, never a partial one.
pub fn write_atomic(dest: &Path, bytes: &[u8], overwrite: bool) -> Result<(), String> {
    let dir = dest
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", dest.display()))?;
    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "bad file name".to_string())?;
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.tmp-{}-{n}", std::process::id()));
    let mut guard = TempGuard {
        path: tmp.clone(),
        armed: true,
    };
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| format!("cannot create {}: {e}", tmp.display()))?;
        restrict(&tmp, false);
        f.write_all(bytes)
            .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("cannot sync {}: {e}", tmp.display()))?;
    }
    if overwrite {
        fs::rename(&tmp, dest).map_err(|e| format!("cannot replace {}: {e}", dest.display()))?;
        guard.armed = false;
    } else {
        match fs::hard_link(&tmp, dest) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(format!("{} already exists", dest.display()));
            }
            Err(e) => return Err(format!("cannot create {}: {e}", dest.display())),
        }
    }
    Ok(())
}

/// Create a folder (and parents) readable only by the current user on Unix.
pub fn make_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    restrict(dir, true);
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &Path, dir: bool) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(
        path,
        fs::Permissions::from_mode(if dir { 0o700 } else { 0o600 }),
    );
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _dir: bool) {}

/// Open a file for appending, creating it readable only by the current user on
/// Unix.
pub fn open_append_private(path: &Path) -> Result<fs::File, String> {
    let mut o = fs::OpenOptions::new();
    o.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))
}

/// Read a whole file as UTF-8. Invalid UTF-8 is an error naming the file,
/// never a lossy read.
pub fn read_strict(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not valid UTF-8", path.display()))
}

/// A slug: `[a-z0-9][a-z0-9-]*`, at most 64 bytes.
pub fn valid_slug(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Join slug segments (`a/b`, at most `max_segments`) under `base`, refusing
/// anything that is not a plain slug and any existing symlink on the way.
pub fn join_id(base: &Path, id: &str, max_segments: usize) -> Result<PathBuf, String> {
    let segs: Vec<&str> = id.split('/').collect();
    if segs.len() > max_segments || !segs.iter().all(|s| valid_slug(s)) {
        return Err(format!(
            "invalid id \"{}\": use lowercase letters, digits and '-' (up to {max_segments} segment(s) joined by '/', each up to 64 characters)",
            clip(id, 60)
        ));
    }
    let mut p = base.to_path_buf();
    for s in &segs {
        p.push(s);
    }
    refuse_symlinks(base, &p)?;
    Ok(p)
}

/// Resolve a user-supplied relative path against `root`. Absolute paths, `..`,
/// and any route that leaves `root` (including through a symlink) are errors.
pub fn project_file(root: &Path, rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() || rel.contains('\0') {
        return Err("path is empty or contains a NUL byte".to_string());
    }
    let p = Path::new(rel);
    if p.is_absolute()
        || p.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err(format!(
            "path \"{}\" must be relative and inside the project",
            clip(rel, 80)
        ));
    }
    let joined = root.join(p);
    let canon_root = root
        .canonicalize()
        .map_err(|e| format!("cannot resolve project folder: {e}"))?;
    match joined.canonicalize() {
        Ok(c) if c.starts_with(&canon_root) => Ok(c),
        Ok(_) => Err(format!(
            "path \"{}\" resolves outside the project",
            clip(rel, 80)
        )),
        Err(_) => Err(format!("path \"{}\" does not exist", clip(rel, 80))),
    }
}

fn refuse_symlinks(base: &Path, full: &Path) -> Result<(), String> {
    let mut cur = base.to_path_buf();
    if let Ok(rest) = full.strip_prefix(base) {
        for c in rest.components() {
            cur.push(c);
            if let Ok(m) = fs::symlink_metadata(&cur) {
                if m.file_type().is_symlink() {
                    return Err(format!(
                        "{} is a symlink; refusing to follow it",
                        cur.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push_str("...");
        t
    }
}

/// Join `segments` under `base`. `.`, `..`, empty segments, and any symlink
/// along the way are refused. The grammar of a segment is the caller's job.
pub fn join_child(base: &Path, segments: &[&str]) -> Result<PathBuf, String> {
    if segments.is_empty() {
        return Err("path is empty".into());
    }
    for s in segments {
        if s.is_empty()
            || *s == "."
            || *s == ".."
            || s.contains('\0')
            || s.contains('/')
            || s.contains('\\')
        {
            return Err(format!("path segment \"{}\" is not allowed", clip(s, 40)));
        }
    }
    let mut p = base.to_path_buf();
    for s in segments {
        p.push(s);
    }
    refuse_symlinks(base, &p)?;
    Ok(p)
}

/// Exclusive lock file beside `subject` (`.name.lock`), created with
/// `create_new`. A lock older than 30 seconds is stale and removed. Drop
/// deletes the file.
pub struct Lock {
    path: PathBuf,
    /// Held open so Windows reports a sharing violation instead of letting a
    /// second creator slip through. Closed before the file is removed.
    file: Option<fs::File>,
}

const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(8);
const LOCK_STALE: std::time::Duration = std::time::Duration::from_secs(30);

impl Lock {
    pub fn acquire(subject: &Path) -> Result<Lock, String> {
        let path = lock_path(subject)?;
        let start = std::time::Instant::now();
        loop {
            match open_lock(&path) {
                Ok(mut file) => {
                    let _ = writeln!(file, "pid {}", std::process::id());
                    let _ = file.sync_all();
                    return Ok(Lock {
                        path,
                        file: Some(file),
                    });
                }
                Err(e) if lock_contended(&e, &path) => {
                    if lock_is_stale(&path) {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if start.elapsed() >= LOCK_WAIT {
                        return Err(format!(
                            "{} is locked by another libellus process. Retry shortly. A lock older than 30 seconds is removed as stale.",
                            path.display()
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(e) => return Err(format!("cannot lock {}: {e}", path.display())),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Unlink while the handle is still open. On Unix the name goes away
        // immediately. On Windows the open allows delete sharing. Try again
        // after the handle is closed, in case the first unlink was refused.
        let _ = fs::remove_file(&self.path);
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

fn open_lock(path: &Path) -> std::io::Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_DELETE. Delete sharing lets Drop unlink
        // the name while this handle is still open.
        opts.share_mode(0x00000001 | 0x00000004);
    }
    opts.open(path)
}

/// Windows can return access-denied (os error 5) instead of already-exists
/// when two creators race, or while the holder still has the file open.
fn lock_contended(err: &std::io::Error, _path: &Path) -> bool {
    // Windows returns access denied (5) or sharing violation (32) for a race
    // on CreateFile, sometimes after the other process has already deleted
    // the name. Treat those as "try again" until the wait expires.
    err.kind() == std::io::ErrorKind::AlreadyExists
        || err.kind() == std::io::ErrorKind::PermissionDenied
        || matches!(err.raw_os_error(), Some(5) | Some(32))
}

fn lock_path(subject: &Path) -> Result<PathBuf, String> {
    let name = subject
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{} has no file name to lock", subject.display()))?;
    let parent = subject
        .parent()
        .ok_or_else(|| format!("{} has no parent folder", subject.display()))?;
    Ok(parent.join(format!(".{name}.lock")))
}

fn lock_is_stale(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return true;
    };
    meta.modified()
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age > LOCK_STALE)
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Current UTC date as `YYYY-MM-DD`.
pub fn today() -> String {
    now_iso()[..10].to_string()
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fsx-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn dates_are_right() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_782), (2026, 11, 25));
    }

    #[test]
    fn create_only_never_overwrites_and_leaves_no_temp() {
        let d = scratch("atomic");
        let f = d.join("a.md");
        write_atomic(&f, b"one", false).unwrap();
        assert!(write_atomic(&f, b"two", false).is_err());
        assert_eq!(fs::read_to_string(&f).unwrap(), "one");
        write_atomic(&f, b"three", true).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "three");
        let strays: Vec<_> = fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n.to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(strays.is_empty(), "{strays:?}");
    }

    #[test]
    fn ids_and_paths_are_guarded() {
        let d = scratch("guard");
        assert!(join_child(&d, &["a", "b"]).is_ok());
        assert!(join_child(&d, &[".."]).is_err());
        assert!(join_child(&d, &["a", "..", "b"]).is_err());
        assert!(join_id(&d, "a/b", 2).is_ok());
        assert!(join_id(&d, "a/b/c", 2).is_err());
        assert!(join_id(&d, "../x", 2).is_err());
        assert!(join_id(&d, "A", 2).is_err());
        assert!(join_id(&d, "", 2).is_err());
        assert!(project_file(&d, "../x").is_err());
        assert!(project_file(&d, "/etc/passwd").is_err());
        fs::write(d.join("f.json"), "{}").unwrap();
        assert!(project_file(&d, "f.json").is_ok());
        assert!(project_file(&d, "missing.json").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_refused() {
        let d = scratch("symlink");
        let outside = scratch("symlink-outside");
        fs::write(outside.join("secret"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, d.join("link")).unwrap();
        assert!(project_file(&d, "link/secret").is_err());
        assert!(join_id(&d, "link", 1).is_err());
    }
}
