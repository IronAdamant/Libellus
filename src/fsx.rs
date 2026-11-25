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

/// Create a folder (and any missing parents) readable only by the current user
/// on Unix. Each directory this call creates is mode 0700, including `.libellus`
/// when `agents` is created under a new store.
pub fn make_private_dir(dir: &Path) -> Result<(), String> {
    let mut created = Vec::new();
    let mut cur = dir.to_path_buf();
    loop {
        if path_exists_nofollow(&cur) {
            break;
        }
        created.push(cur.clone());
        if !cur.pop() {
            break;
        }
    }
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    restrict(dir, true);
    for path in &created {
        restrict(path, true);
    }
    Ok(())
}

/// True when something (a file, folder or symlink) is at `path`. A symlink
/// is not followed.
pub fn path_exists_nofollow(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
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

/// What `symlink_metadata` says. A symlink is not followed, and a fifo, socket
/// or device is `Other` so callers can refuse to open it.
pub enum FileKind {
    Missing,
    Regular { len: u64 },
    Symlink,
    Other,
}

pub fn file_kind(path: &Path) -> Result<FileKind, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(FileKind::Missing),
        Err(e) => Err(format!("cannot stat {}: {e}", path.display())),
        Ok(meta) => {
            let kind = meta.file_type();
            if kind.is_symlink() {
                Ok(FileKind::Symlink)
            } else if kind.is_file() {
                Ok(FileKind::Regular { len: meta.len() })
            } else {
                Ok(FileKind::Other)
            }
        }
    }
}

/// A missing path is fine (the caller may create it). A symlink or anything
/// that is not a regular file is an error, before any open that could block.
pub fn refuse_special(path: &Path) -> Result<(), String> {
    match file_kind(path)? {
        FileKind::Missing | FileKind::Regular { .. } => Ok(()),
        FileKind::Symlink => Err(format!(
            "{} is a symlink; refusing to follow it",
            path.display()
        )),
        FileKind::Other => Err(format!(
            "{} is not a regular file; refusing to open it",
            path.display()
        )),
    }
}

/// Read a whole file as UTF-8. Invalid UTF-8 is an error naming the file,
/// never a lossy read. Callers must refuse a symlink or a non-regular file
/// first: opening a fifo blocks forever.
pub fn read_strict(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    String::from_utf8(bytes).map_err(|_| format!("{} is not valid UTF-8", path.display()))
}

/// `YYYY-MM-DDTHH:MM:SSZ`, a real civil date and a clock time.
pub fn valid_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return false;
    }
    let num = |i: usize, n: usize| s[i..i + n].bytes().all(|c| c.is_ascii_digit());
    if !(num(0, 4) && num(5, 2) && num(8, 2) && num(11, 2) && num(14, 2) && num(17, 2)) {
        return false;
    }
    let y: i64 = s[0..4].parse().unwrap_or(0);
    let m: i64 = s[5..7].parse().unwrap_or(0);
    let d: i64 = s[8..10].parse().unwrap_or(0);
    let hh: i64 = s[11..13].parse().unwrap_or(99);
    let mm: i64 = s[14..16].parse().unwrap_or(99);
    let ss: i64 = s[17..19].parse().unwrap_or(99);
    (1..=12).contains(&m)
        && (1..=days_in_month(y, m)).contains(&d)
        && (0..24).contains(&hh)
        && (0..60).contains(&mm)
        && (0..60).contains(&ss)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
        2 => 28,
        _ => 0,
    }
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

/// A name Windows reserves for a device (`con`, `nul`, `com1` and so on),
/// with or without an extension. Such a file cannot be made or opened on
/// Windows, so it is refused on every platform to keep a store portable.
pub fn reserved_name(seg: &str) -> bool {
    let stem = seg.split('.').next().unwrap_or(seg).to_ascii_lowercase();
    match stem.as_str() {
        "con" | "prn" | "aux" | "nul" => true,
        _ => {
            let b = stem.as_bytes();
            b.len() == 4
                && (stem.starts_with("com") || stem.starts_with("lpt"))
                && (b'1'..=b'9').contains(&b[3])
        }
    }
}

/// Join slug segments (`a/b`, at most `max_segments`) under `base`, refusing
/// anything that is not a plain slug, a Windows device name, and any existing
/// symlink on the way.
pub fn join_id(base: &Path, id: &str, max_segments: usize) -> Result<PathBuf, String> {
    let segs: Vec<&str> = id.split('/').collect();
    if segs.len() > max_segments || !segs.iter().all(|s| valid_slug(s)) {
        return Err(format!(
            "invalid id \"{}\": use lowercase letters, digits and '-' (up to {max_segments} segment(s) joined by '/', each up to 64 characters)",
            clip(id, 60)
        ));
    }
    if let Some(bad) = segs.iter().find(|s| reserved_name(s)) {
        return Err(format!(
            "invalid id \"{}\": \"{bad}\" is a device name on Windows (con, prn, aux, nul, com1-9, lpt1-9); pick another name",
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

/// A slug made from free text (a summary or a title): lowercase letters,
/// digits and single dashes, at most 48 bytes. `n` above 1 adds `-n`. Empty
/// text gives `fallback`. A Windows device name gains `-<fallback>`.
pub fn slug_from(text: &str, n: u32, fallback: &str) -> String {
    let mut slug: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    slug.truncate(48);
    let slug = slug.trim_matches('-').to_string();
    let slug = if slug.is_empty() {
        fallback.to_string()
    } else if reserved_name(&slug) {
        format!("{slug}-{fallback}")
    } else {
        slug
    };
    if n <= 1 {
        slug
    } else {
        format!("{slug}-{n}")
    }
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
    /// The line this holder wrote. Drop unlinks the name only while it still
    /// holds this line, so a lock that was broken and taken by another writer
    /// is left alone.
    text: String,
    /// Held open so Windows reports a sharing violation instead of letting a
    /// second creator slip through. Closed before the file is removed.
    file: Option<fs::File>,
}

const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(8);
pub(crate) const LOCK_STALE: std::time::Duration = std::time::Duration::from_secs(30);
/// How long a lock may exist before its pid line is finished. A writer killed
/// in that window used to block other writers for `LOCK_STALE`.
pub(crate) const LOCK_BARE: std::time::Duration = std::time::Duration::from_secs(1);

impl Lock {
    pub fn acquire(subject: &Path) -> Result<Lock, String> {
        let path = lock_path(subject)?;
        let start = std::time::Instant::now();
        loop {
            match open_lock(&path) {
                Ok(mut file) => {
                    let text = format!("pid {} {}\n", std::process::id(), lock_token());
                    let _ = file.write_all(text.as_bytes());
                    let _ = file.sync_all();
                    return Ok(Lock {
                        path,
                        text,
                        file: Some(file),
                    });
                }
                Err(e) if lock_contended(&e, &path) => {
                    if break_stale_lock(&path) {
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
        // Unlink once, while this handle is still open, then close. A second
        // unlink after the close can delete a lock the next writer just created.
        // A name that no longer holds this writer's line belongs to someone else.
        if fs::read_to_string(&self.path).is_ok_and(|t| t == self.text) {
            let _ = fs::remove_file(&self.path);
        }
        self.file.take();
    }
}

/// A token unique to one acquire in this process, so two holders never write
/// the same lock line.
fn lock_token() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}-{n}")
}

/// How long the `.break` guard may stay before it is taken as abandoned.
/// Breaking takes microseconds, so only a writer killed mid-break leaves one.
const BREAK_STALE: std::time::Duration = std::time::Duration::from_secs(5);

/// Remove the lock at `path` if it is stale, and say whether it was removed.
/// Only the holder of the `.break` guard beside it removes, and it judges the
/// lock again while holding the guard. Nobody can create the lock name while
/// the stale file is there, so the file judged stale is the file removed: a
/// lock created after another contender's check is never deleted.
pub(crate) fn break_stale_lock(path: &Path) -> bool {
    if !lock_is_stale(path) {
        return false;
    }
    let mut guard = path.as_os_str().to_owned();
    guard.push(".break");
    let guard = PathBuf::from(guard);
    let held = match open_lock(&guard) {
        Ok(file) => file,
        Err(_) => {
            let old = fs::symlink_metadata(&guard)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > BREAK_STALE);
            if old {
                let _ = fs::remove_file(&guard);
            }
            return false;
        }
    };
    let removed = lock_is_stale(path) && fs::remove_file(path).is_ok();
    drop(held);
    let _ = fs::remove_file(&guard);
    removed
}

fn open_lock(path: &Path) -> std::io::Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
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
    if err.kind() == std::io::ErrorKind::AlreadyExists {
        return true;
    }
    // On Unix, permission denied is a real error (a read-only folder), not a
    // race. Waiting on it would blame another process after eight seconds.
    cfg!(windows)
        && (err.kind() == std::io::ErrorKind::PermissionDenied
            || matches!(err.raw_os_error(), Some(5) | Some(32)))
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

pub(crate) fn lock_is_stale(path: &Path) -> bool {
    // Missing means the holder already unlinked it. Do not treat that as
    // stale: the caller would otherwise delete a lock created a moment later.
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    let age = meta.modified().ok().and_then(|t| t.elapsed().ok());
    let future = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::SystemTime::now()).ok());
    if age.is_some_and(|a| a > LOCK_STALE) {
        return true;
    }
    let text = fs::read_to_string(path).unwrap_or_default();
    if text.ends_with('\n') {
        let Some(pid) = lock_pid_text(&text) else {
            // A finished line that is not a pid ("held", for example) is a
            // live lock until it is old enough to be stale.
            return false;
        };
        if pid_alive(Some(pid)) != Some(false) {
            return false;
        }
        // The name may have been replaced since the first read. Delete only
        // when the same finished pid line is still there and still dead.
        let Ok(again) = fs::read_to_string(path) else {
            return false;
        };
        return again == text && pid_alive(Some(pid)) == Some(false);
    }
    // No finished pid line. The holder writes that line immediately after
    // creating the file. A kill before the write leaves this shape. A future
    // mtime cannot be aged, so it does not get to block for an hour.
    age.is_some_and(|a| a > LOCK_BARE)
        || future.is_some_and(|d| d > std::time::Duration::from_secs(2))
}

fn lock_pid_text(text: &str) -> Option<u32> {
    let rest = text.trim().strip_prefix("pid ")?;
    rest.split_whitespace().next()?.parse().ok()
}

/// `Some(false)` when this platform can see that `pid` is not running.
///
/// On Unix this asks the kernel with `kill(pid, 0)`, which sends no signal.
/// Success or EPERM (the process exists but belongs to someone else) is
/// alive, ESRCH is dead, and any other error is unknown. It does not run
/// `ps`, so a `ps` without `-p` cannot make a live holder look dead. On other
/// platforms the answer is unknown and locks are judged by age.
pub(crate) fn pid_alive(pid: Option<u32>) -> Option<bool> {
    let pid = pid?;
    #[cfg(unix)]
    {
        extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        // 0 and negative values name process groups, not one process. No
        // single process with such an id can hold a lock.
        let Ok(id) = i32::try_from(pid) else {
            return Some(false);
        };
        if id <= 0 {
            return Some(false);
        }
        // SAFETY: signal 0 only checks that the process exists.
        if unsafe { kill(id, 0) } == 0 {
            return Some(true);
        }
        const EPERM: i32 = 1;
        const ESRCH: i32 = 3;
        match std::io::Error::last_os_error().raw_os_error() {
            Some(EPERM) => Some(true),
            Some(ESRCH) => Some(false),
            _ => None,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// Lock files under `root` whose holder is gone or whose age is past
/// `LOCK_STALE`. The message says how to clear one.
pub fn stale_lock_notes(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect_stale_locks(root, &mut out);
    out
}

fn collect_stale_locks(dir: &Path, out: &mut Vec<String>) {
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
            collect_stale_locks(&path, out);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') && name.ends_with(".lock") && lock_is_stale(&path) {
            out.push(format!(
                "stale lock {}. Remove that file if no libellus process is using it.",
                path.display()
            ));
        }
    }
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
    fn the_lock_keeps_one_holder() {
        let d = scratch("lock");
        let subject = d.join("ledger.jsonl");
        fs::write(&subject, b"\n").unwrap();
        let inside = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let subject = subject.clone();
            let inside = inside.clone();
            handles.push(std::thread::spawn(move || {
                for _ in 0..20 {
                    let _lock = Lock::acquire(&subject).unwrap();
                    let n = inside.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    assert_eq!(n, 0, "two holders");
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    inside.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn racing_past_a_dead_lock_leaves_one_holder() {
        let d = scratch("lock-dead");
        let subject = d.join("ledger.jsonl");
        fs::write(&subject, b"\n").unwrap();
        use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
        for _ in 0..12 {
            // Windows cannot see that a pid is gone, so there only the age
            // makes the lock stale. Unix keeps the dead-pid path, whose double
            // read is the widest race window.
            fs::write(d.join(".ledger.jsonl.lock"), "pid 999999\n").unwrap();
            #[cfg(windows)]
            fs::File::options()
                .write(true)
                .open(d.join(".ledger.jsonl.lock"))
                .unwrap()
                .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(120))
                .unwrap();
            let inside = std::sync::Arc::new(AtomicU64::new(0));
            let most = std::sync::Arc::new(AtomicU64::new(0));
            let gate = std::sync::Arc::new(std::sync::Barrier::new(6));
            let handles: Vec<_> = (0..6)
                .map(|_| {
                    let (subject, inside, most, gate) =
                        (subject.clone(), inside.clone(), most.clone(), gate.clone());
                    std::thread::spawn(move || {
                        gate.wait();
                        let _lock = Lock::acquire(&subject).unwrap();
                        most.fetch_max(inside.fetch_add(1, SeqCst) + 1, SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(30));
                        inside.fetch_sub(1, SeqCst);
                    })
                })
                .collect();
            for handle in handles {
                handle.join().unwrap();
            }
            assert_eq!(
                most.load(SeqCst),
                1,
                "two holders after a stale lock was broken"
            );
            assert!(!d.join(".ledger.jsonl.lock").exists());
            assert!(!d.join(".ledger.jsonl.lock.break").exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_lock_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("lock-mode");
        let subject = d.join("ledger.jsonl");
        let _lock = Lock::acquire(&subject).unwrap();
        let mode = fs::metadata(d.join(".ledger.jsonl.lock"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn dates_are_right() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_782), (2026, 11, 25));
        assert!(valid_timestamp("2026-11-25T00:00:00Z"));
        assert!(valid_timestamp("2024-02-29T23:59:59Z"));
        assert!(!valid_timestamp("2026-02-31T00:00:00Z"));
        assert!(!valid_timestamp("2025-02-29T00:00:00Z"));
        assert!(!valid_timestamp("2026-11-25"));
        assert!(!valid_timestamp("t"));
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
