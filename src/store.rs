//! Where the private store lives: the nearest `.libellus/` walking up from the
//! working directory, or `$LIBELLUS_DIR`.
//!
//! ```text
//! .libellus/
//!   agents/<agent>/notes/<id>.md     one private note per file
//!   agents/<agent>/ledger.jsonl      append-only number ledger
//! ```

use crate::fsx;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Store {
    /// The `.libellus` folder.
    pub dir: PathBuf,
    /// The folder that holds it; where AGENTS.md is looked for.
    pub project: PathBuf,
}

/// `$LIBELLUS_DIR`, when it is set to something other than blank. An empty
/// value is treated as unset, like the other Libellus variables.
fn env_dir() -> Option<PathBuf> {
    let v = std::env::var_os("LIBELLUS_DIR")?;
    if v.to_str().is_some_and(|s| s.trim().is_empty()) {
        return None;
    }
    Some(PathBuf::from(v))
}

impl Store {
    pub fn open() -> Result<Store, String> {
        if let Some(dir) = env_dir() {
            return Store::at(dir);
        }
        let cwd =
            std::env::current_dir().map_err(|e| format!("cannot read the working folder: {e}"))?;
        let mut cur: &Path = &cwd;
        loop {
            let cand = cur.join(".libellus");
            if let Ok(m) = fs::symlink_metadata(&cand) {
                if m.is_dir() {
                    return Store::at(cand);
                }
                // A symlink or file here is this folder's store gone wrong.
                // Walking past it would write into a parent's store instead.
                return Err(format!(
                    "{} is not a folder (a symlink or file); refusing to use it or a store above it",
                    cand.display()
                ));
            }
            match cur.parent() {
                Some(p) => cur = p,
                None => break,
            }
        }
        Err("no .libellus folder here or above. Run `libellus init` in the project folder, or set LIBELLUS_DIR".to_string())
    }

    fn at(dir: PathBuf) -> Result<Store, String> {
        match fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "{} is not a folder (or is a symlink)",
                    dir.display()
                ))
            }
            Err(_) => {
                return Err(format!(
                    "{} does not exist. Run `libellus init`",
                    dir.display()
                ))
            }
        }
        match fs::symlink_metadata(dir.join("agents")) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            _ => {
                return Err(format!(
                    "{} has no agents folder. Run `libellus init`",
                    dir.display()
                ))
            }
        }
        let project = dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Store { dir, project })
    }

    /// Create `.libellus/` in the working directory. Safe to repeat.
    pub fn init() -> Result<Store, String> {
        let dir = match env_dir() {
            Some(dir) => dir,
            None => std::env::current_dir()
                .map_err(|e| format!("cannot read the working folder: {e}"))?
                .join(".libellus"),
        };
        fsx::make_private_dir(&dir.join("agents"))?;
        Store::at(dir)
    }

    pub fn agent_dir(&self, agent: &str) -> Result<PathBuf, String> {
        if !fsx::valid_slug(agent) {
            return Err(format!(
                "invalid agent \"{}\": use lowercase letters, digits and '-' (for example main or sub-2)",
                fsx::clip(agent, 40)
            ));
        }
        if fsx::reserved_name(agent) {
            return Err(format!(
                "invalid agent \"{agent}\": a device name on Windows (con, prn, aux, nul, com1-9, lpt1-9); pick another name"
            ));
        }
        fsx::join_id(&self.dir.join("agents"), agent, 1)
    }

    pub fn notes_dir(&self, agent: &str) -> Result<PathBuf, String> {
        fsx::join_child(&self.agent_dir(agent)?, &["notes"])
    }

    /// `agents/<agent>/archive`, refused when it is a symlink.
    pub fn archive_dir(&self, agent: &str) -> Result<PathBuf, String> {
        fsx::join_child(&self.agent_dir(agent)?, &["archive"])
    }

    pub fn ledger_path(&self, agent: &str) -> Result<PathBuf, String> {
        Ok(self.agent_dir(agent)?.join("ledger.jsonl"))
    }

    /// Agent names that have a folder, sorted.
    pub fn agents(&self) -> Result<Vec<String>, String> {
        let base = self.dir.join("agents");
        let mut out = Vec::new();
        let rd = match fs::read_dir(&base) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(format!("cannot read {}: {e}", base.display())),
        };
        for e in rd {
            let e = e.map_err(|e| format!("cannot read {}: {e}", base.display()))?;
            let name = e.file_name();
            let Some(n) = name.to_str() else {
                continue;
            };
            let kind = e
                .file_type()
                .map_err(|err| format!("cannot stat {}: {err}", e.path().display()))?;
            if kind.is_symlink() {
                return Err(format!(
                    "{} is a symlink; refusing to follow it",
                    e.path().display()
                ));
            }
            if kind.is_dir() && fsx::valid_slug(n) {
                out.push(n.to_string());
            }
        }
        out.sort();
        Ok(out)
    }
}

/// The folder whose AGENTS.md chooses the tool groups. With a store, that is
/// the folder holding it. Before `libellus init`, it is the parent of
/// `$LIBELLUS_DIR` when that is set, otherwise the nearest folder walking up
/// from the working directory that has an AGENTS.md, the same walk that
/// finds a store.
pub fn project_dir() -> Option<PathBuf> {
    if let Ok(store) = Store::open() {
        return Some(store.project);
    }
    if let Some(dir) = env_dir() {
        return dir.parent().map(Path::to_path_buf);
    }
    let cwd = std::env::current_dir().ok()?;
    let mut cur: &Path = &cwd;
    loop {
        if fs::symlink_metadata(cur.join("AGENTS.md")).is_ok() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
}
