//! Operating-system file locks, standard library only.
//!
//! A lock is an exclusive lock on a small lock file that is opened (and
//! created when missing, mode 0600 on Unix) and kept open for the lock's
//! life: `flock(LOCK_EX | LOCK_NB)` on Unix, `LockFileEx` with
//! `LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY` on Windows. The
//! kernel releases the lock when the holder closes the file, exits or is
//! killed, so there is no stale lock to judge: no pid, no age, no token.
//!
//! Rules for callers:
//!
//! - Never delete a lock file. If it is deleted while held, the next writer
//!   creates a new file at that name and locks that, and two writers hold
//!   "the" lock. The file is empty and costs nothing to keep.
//! - Locks are per open file, not per process: two `FileLock`s on the same
//!   path in one process exclude each other, and taking one twice on one
//!   thread waits for itself until the limit. (This is `flock`, not
//!   `fcntl`: closing some other descriptor of the file does not drop it.)
//! - std opens files close-on-exec and Windows handles uninheritable, so a
//!   child process never inherits a held lock.
//! - On some NFS and SMB setups locks are emulated or not shared between
//!   machines. `network_fs` names such a file system so a caller can warn.
//!
//! The file depends on nothing outside `std`, so another crate can copy it
//! as is. When the minimum Rust reaches 1.89, `std::fs::File::try_lock`
//! does the same job.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// An exclusive lock, held until drop.
#[derive(Debug)]
pub struct FileLock {
    file: File,
    path: PathBuf,
}

impl FileLock {
    /// Take the lock at `path` now, or `Ok(None)` when another holder has it.
    pub fn try_acquire(path: &Path) -> Result<Option<FileLock>, String> {
        let file = open(path)?;
        loop {
            match sys::try_lock(&file) {
                Ok(()) => {
                    return Ok(Some(FileLock {
                        file,
                        path: path.to_path_buf(),
                    }))
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) if sys::busy(&e) => return Ok(None),
                Err(e) => return Err(format!("cannot lock {}: {e}", path.display())),
            }
        }
    }

    /// Take the lock at `path`, retrying with a short sleep for up to `wait`.
    /// `holder` names the program in the busy error ("locked by another
    /// `holder` process").
    pub fn acquire(path: &Path, wait: Duration, holder: &str) -> Result<FileLock, String> {
        let start = Instant::now();
        let mut pause = Duration::from_millis(1);
        loop {
            if let Some(lock) = FileLock::try_acquire(path)? {
                return Ok(lock);
            }
            if start.elapsed() >= wait {
                return Err(format!(
                    "{} is locked by another {holder} process. Retry shortly.",
                    path.display()
                ));
            }
            std::thread::sleep(pause);
            pause = (pause * 2).min(Duration::from_millis(10));
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        // Closing the file releases the lock too; unlocking first makes the
        // release immediate on Windows, where a close may be lazy.
        sys::unlock(&self.file);
    }
}

/// Open the lock file read-write, creating it when missing. A symlink or a
/// non-regular file at `path` is refused rather than followed.
fn open(path: &Path) -> Result<File, String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {
            return Err(format!(
                "lock file {} is a symlink; refusing to follow it",
                path.display()
            ))
        }
        Ok(m) if !m.is_file() => {
            return Err(format!(
                "lock file {} is not a regular file; refusing to open it",
                path.display()
            ))
        }
        _ => {}
    }
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)
        .map_err(|e| format!("cannot open lock file {}: {e}", path.display()))
}

#[cfg(unix)]
mod sys {
    use std::fs::File;
    use std::io;
    use std::os::unix::io::AsRawFd;

    // <sys/file.h>: the same values on macOS, Linux (every architecture) and
    // the BSDs.
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    const LOCK_UN: i32 = 8;

    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }

    pub fn try_lock(file: &File) -> io::Result<()> {
        // SAFETY: the descriptor is open for the life of `file`.
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn unlock(file: &File) {
        // SAFETY: as above.
        unsafe { flock(file.as_raw_fd(), LOCK_UN) };
    }

    /// EWOULDBLOCK, which is EAGAIN on macOS (35) and Linux (11); std maps
    /// both to `WouldBlock`.
    pub fn busy(e: &io::Error) -> bool {
        e.kind() == io::ErrorKind::WouldBlock
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::c_void;
    use std::fs::File;
    use std::io;
    use std::os::windows::io::AsRawHandle;

    // minwinbase.h / winerror.h, the values std's own bindings use.
    const LOCKFILE_FAIL_IMMEDIATELY: u32 = 0x1;
    const LOCKFILE_EXCLUSIVE_LOCK: u32 = 0x2;
    const ERROR_LOCK_VIOLATION: i32 = 33;
    const ERROR_IO_PENDING: i32 = 997;

    /// OVERLAPPED: two ULONG_PTRs, the Offset/OffsetHigh union (8 bytes),
    /// then the event handle. 32 bytes on x64. Zeroed, it names offset 0.
    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        h_event: *mut c_void,
    }

    #[cfg(target_pointer_width = "64")]
    const _: () = assert!(std::mem::size_of::<Overlapped>() == 32);

    #[link(name = "kernel32")]
    extern "system" {
        fn LockFileEx(
            hfile: *mut c_void,
            dwflags: u32,
            dwreserved: u32,
            nnumberofbytestolocklow: u32,
            nnumberofbytestolockhigh: u32,
            lpoverlapped: *mut Overlapped,
        ) -> i32;
        fn UnlockFileEx(
            hfile: *mut c_void,
            dwreserved: u32,
            nnumberofbytestounlocklow: u32,
            nnumberofbytestounlockhigh: u32,
            lpoverlapped: *mut Overlapped,
        ) -> i32;
    }

    fn overlapped() -> Overlapped {
        Overlapped {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            h_event: std::ptr::null_mut(),
        }
    }

    pub fn try_lock(file: &File) -> io::Result<()> {
        let mut ov = overlapped();
        // SAFETY: the handle is open for the life of `file`; `ov` outlives
        // the call, which does not keep it (the handle is synchronous).
        let ok = unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                u32::MAX,
                u32::MAX,
                &mut ov,
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn unlock(file: &File) {
        let mut ov = overlapped();
        // SAFETY: as above; the range matches the one locked.
        unsafe { UnlockFileEx(file.as_raw_handle(), 0, u32::MAX, u32::MAX, &mut ov) };
    }

    pub fn busy(e: &io::Error) -> bool {
        matches!(
            e.raw_os_error(),
            Some(ERROR_LOCK_VIOLATION) | Some(ERROR_IO_PENDING)
        )
    }
}

#[cfg(not(any(unix, windows)))]
mod sys {
    use std::fs::File;
    use std::io;

    pub fn try_lock(_file: &File) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "file locks are not supported on this platform",
        ))
    }

    pub fn unlock(_file: &File) {}

    pub fn busy(_e: &io::Error) -> bool {
        false
    }
}

/// The name of the network file system `path` is on (`nfs`, `smbfs`,
/// `cifs`, `remote drive`, ...), or `None` when it is local or this
/// platform cannot tell cheaply. macOS reads `statfs` (`MNT_LOCAL` and
/// `f_fstypename`), Linux on x86_64 and aarch64 reads `statfs` `f_type`,
/// Windows asks `GetDriveTypeW` for the drive root (a UNC path is remote).
pub fn network_fs(path: &Path) -> Option<String> {
    net::network_fs(path)
}

#[cfg(target_os = "macos")]
mod net {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    const MNT_LOCAL: u32 = 0x0000_1000;

    /// `struct statfs` with 64-bit inodes (<sys/mount.h>), 2168 bytes.
    #[repr(C)]
    struct StatFs {
        f_bsize: u32,
        f_iosize: i32,
        f_blocks: u64,
        f_bfree: u64,
        f_bavail: u64,
        f_files: u64,
        f_ffree: u64,
        f_fsid: [i32; 2],
        f_owner: u32,
        f_type: u32,
        f_flags: u32,
        f_fssubtype: u32,
        f_fstypename: [u8; 16],
        f_mntonname: [u8; 1024],
        f_mntfromname: [u8; 1024],
        f_flags_ext: u32,
        f_reserved: [u32; 7],
    }

    const _: () = assert!(std::mem::size_of::<StatFs>() == 2168);

    extern "C" {
        // On x86_64 the plain symbol is the old 32-bit inode layout.
        #[cfg_attr(target_arch = "x86_64", link_name = "statfs$INODE64")]
        fn statfs(path: *const std::ffi::c_char, buf: *mut StatFs) -> i32;
    }

    pub fn network_fs(path: &Path) -> Option<String> {
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut buf = std::mem::MaybeUninit::<StatFs>::zeroed();
        // SAFETY: `c` is NUL-terminated and `buf` is the full struct.
        if unsafe { statfs(c.as_ptr(), buf.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: statfs filled it; every bit pattern is valid anyway.
        let s = unsafe { buf.assume_init() };
        let end = s.f_fstypename.iter().position(|&b| b == 0).unwrap_or(16);
        let name = String::from_utf8_lossy(&s.f_fstypename[..end]).into_owned();
        // An autofs trigger point is not local, but what mounts on it is
        // judged on its own once the path is reached.
        if s.f_flags & MNT_LOCAL != 0 || name == "autofs" {
            return None;
        }
        Some(name)
    }
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod net {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    /// `struct statfs` on 64-bit Linux: fifteen 8-byte words, `f_type` first.
    #[repr(C)]
    struct StatFs {
        f_type: i64,
        rest: [i64; 14],
    }

    extern "C" {
        fn statfs(path: *const std::ffi::c_char, buf: *mut StatFs) -> i32;
    }

    /// `f_type` magic numbers from <linux/magic.h> and the file systems.
    const NETWORK: &[(i64, &str)] = &[
        (0x6969, "nfs"),
        (0x517B, "smb"),
        (0xFF53_4D42, "cifs"),
        (0xFE53_4D42, "smb2"),
        (0x0102_1997, "9p"),
        (0x5346_414F, "afs"),
        (0x7375_7245, "coda"),
        (0x00C3_6400, "ceph"),
    ];

    pub fn network_fs(path: &Path) -> Option<String> {
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut buf = std::mem::MaybeUninit::<StatFs>::zeroed();
        // SAFETY: `c` is NUL-terminated and `buf` is the full struct.
        if unsafe { statfs(c.as_ptr(), buf.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: statfs filled it; every bit pattern is valid anyway.
        let kind = unsafe { buf.assume_init() }.f_type;
        NETWORK
            .iter()
            .find(|(magic, _)| *magic == kind)
            .map(|(_, name)| (*name).to_string())
    }
}

#[cfg(windows)]
mod net {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Path, Prefix};

    const DRIVE_REMOTE: u32 = 4;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetDriveTypeW(lprootpathname: *const u16) -> u32;
    }

    pub fn network_fs(path: &Path) -> Option<String> {
        let letter = match path.components().next()? {
            Component::Prefix(p) => match p.kind() {
                Prefix::UNC(..) | Prefix::VerbatimUNC(..) => {
                    return Some("network share".to_string())
                }
                Prefix::Disk(l) | Prefix::VerbatimDisk(l) => l,
                _ => return None,
            },
            _ => return None,
        };
        let root: Vec<u16> = std::ffi::OsStr::new(&format!("{}:\\", letter as char))
            .encode_wide()
            .chain(Some(0))
            .collect();
        // SAFETY: `root` is a NUL-terminated wide string.
        if unsafe { GetDriveTypeW(root.as_ptr()) } == DRIVE_REMOTE {
            Some("remote drive".to_string())
        } else {
            None
        }
    }
}

#[cfg(not(any(
    target_os = "macos",
    windows,
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
)))]
mod net {
    pub fn network_fs(_path: &std::path::Path) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("oslock-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn a_held_lock_is_busy_until_dropped_and_the_file_stays() {
        let d = scratch("busy");
        let path = d.join("x.lock");
        let held = FileLock::try_acquire(&path).unwrap().unwrap();
        assert!(FileLock::try_acquire(&path).unwrap().is_none());
        let err = FileLock::acquire(&path, Duration::from_millis(50), "test").unwrap_err();
        assert!(err.contains("locked by another test process"), "{err}");
        drop(held);
        assert!(FileLock::try_acquire(&path).unwrap().is_some());
        assert!(path.is_file(), "a lock file is never deleted");
    }

    #[test]
    fn one_holder_among_threads() {
        use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
        let d = scratch("threads");
        let path = d.join("x.lock");
        let inside = std::sync::Arc::new(AtomicU64::new(0));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (path, inside) = (path.clone(), inside.clone());
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        let _l = FileLock::acquire(&path, Duration::from_secs(20), "test").unwrap();
                        assert_eq!(inside.fetch_add(1, SeqCst), 0, "two holders");
                        std::thread::yield_now();
                        inside.fetch_sub(1, SeqCst);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn leftover_contents_do_not_matter() {
        let d = scratch("old");
        let path = d.join("x.lock");
        fs::write(&path, "pid 999999 old-token\n").unwrap();
        assert!(FileLock::try_acquire(&path).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_refused_and_a_new_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("link");
        let path = d.join("x.lock");
        let _l = FileLock::try_acquire(&path).unwrap().unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let link = d.join("y.lock");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(FileLock::try_acquire(&link)
            .unwrap_err()
            .contains("symlink"));
    }

    #[test]
    fn the_temp_folder_is_not_a_network_drive() {
        assert_eq!(network_fs(&std::env::temp_dir()), None);
    }
}
