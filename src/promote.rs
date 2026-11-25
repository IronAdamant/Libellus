//! Promote one private note into a Relatum store.
//!
//! Relatum is not run and its crate is not copied. This writes the same
//! markdown Relatum would: flat frontmatter (`kind`, `summary`, `author`,
//! `updated`, `tags`) and a create-only file under `.swarmnotes/`. The store
//! is `$SWARMNOTES_DIR`, else the nearest `.swarmnotes/` walking up. Nothing
//! here creates that folder.

use crate::fsx;
use crate::json::Value;
use crate::note::{self, Note};
use crate::store::Store;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub fn promote(agent: &str, id: &str, scope: &str) -> Result<Value, String> {
    let store = Store::open()?;
    let mut note = note::load_one(&store, agent, id)?;
    if let Some(existing) = &note.promoted {
        return Err(format!("note \"{id}\" is already promoted as {existing}"));
    }
    let kind = relatum_kind(&note)?;
    let root = find_relatum()?;
    let base = id_segments(scope, id)?;
    let (rel_id, rel_path) = write_relatum(&root, &base, &note, agent, kind)?;
    note.promoted = Some(rel_id.clone());
    let note_path = note::note_path(&store, agent, id)?;
    if let Err(e) = fsx::write_atomic(&note_path, note.render().as_bytes(), true) {
        return Err(format!(
            "Relatum note written at {} but the Libellus note at {} was not updated ({e}). promoted is still unset. Remove the Relatum file before retrying, or the retry will choose a new id.",
            rel_path.display(),
            note_path.display()
        ));
    }
    Ok(Value::obj(vec![
        ("id", Value::str(id)),
        ("promoted", Value::str(&rel_id)),
        ("relatum_kind", Value::str(kind)),
        ("path", Value::String(rel_path.display().to_string())),
    ]))
}

fn relatum_kind(note: &Note) -> Result<&'static str, String> {
    match note.kind.as_str() {
        "question" if note.status != "resolved" => Err(format!(
            "note \"{}\" is an open question; resolve it before promoting",
            note.id
        )),
        "question" => Ok("finding"),
        "decision" => Ok("decision"),
        "context" => Ok("context"),
        "finding" => Ok("finding"),
        "dead-end" => Ok("attempt"),
        other => Err(format!("note kind \"{other}\" cannot be promoted")),
    }
}

/// `$SWARMNOTES_DIR`, else the nearest `.swarmnotes/` above the working
/// directory. A missing store is an error and is not created.
fn find_relatum() -> Result<PathBuf, String> {
    if let Some(env) = std::env::var_os("SWARMNOTES_DIR") {
        if !env.is_empty() {
            let raw = env.to_string_lossy().to_string();
            let dir = PathBuf::from(env);
            if !dir.is_dir() {
                return Err(format!("SWARMNOTES_DIR={raw} is not a directory"));
            }
            return Ok(dir);
        }
    }
    let cwd =
        std::env::current_dir().map_err(|e| format!("cannot read the working folder: {e}"))?;
    let mut cur = cwd.as_path();
    loop {
        let cand = cur.join(".swarmnotes");
        if cand.is_dir() {
            return Ok(cand);
        }
        match cur.parent() {
            Some(parent) => cur = parent,
            None => break,
        }
    }
    Err("no .swarmnotes/ found. Run `relatum init`, or set SWARMNOTES_DIR".into())
}

fn id_segments(scope: &str, id: &str) -> Result<Vec<String>, String> {
    reject_escape(scope)?;
    let scope = scope.trim_matches('/');
    if scope.is_empty() {
        return Err("scope is empty".into());
    }
    let mut segs = Vec::new();
    for part in scope.split('/') {
        if !valid_relatum_segment(part) {
            return Err(format!(
                "invalid scope \"{}\": each segment must match _?[a-z0-9][a-z0-9-]*",
                fsx::clip(scope, 60)
            ));
        }
        segs.push(part.to_string());
    }
    for part in id.split('/') {
        if !valid_relatum_segment(part) {
            return Err(format!(
                "invalid id \"{}\": each segment must match _?[a-z0-9][a-z0-9-]*",
                fsx::clip(id, 60)
            ));
        }
        segs.push(part.to_string());
    }
    Ok(segs)
}

fn reject_escape(scope: &str) -> Result<(), String> {
    if scope.is_empty() || scope.contains('\0') || scope.contains('\\') {
        return Err(format!(
            "invalid scope \"{}\": use slug segments inside the Relatum store",
            fsx::clip(scope, 60)
        ));
    }
    let path = Path::new(scope);
    if path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir
                    | Component::Prefix(_)
                    | Component::RootDir
                    | Component::CurDir
            )
        })
    {
        return Err(format!(
            "scope \"{}\" must stay inside the Relatum store",
            fsx::clip(scope, 80)
        ));
    }
    Ok(())
}

/// Relatum id segment: `_?[a-z0-9][a-z0-9-]*`.
fn valid_relatum_segment(seg: &str) -> bool {
    let b = seg.as_bytes();
    if b.is_empty() {
        return false;
    }
    let mut i = 0;
    if b[0] == b'_' {
        i = 1;
    }
    if i >= b.len() || !(b[i].is_ascii_lowercase() || b[i].is_ascii_digit()) {
        return false;
    }
    i += 1;
    while i < b.len() {
        if !(b[i].is_ascii_lowercase() || b[i].is_ascii_digit() || b[i] == b'-') {
            return false;
        }
        i += 1;
    }
    true
}

fn write_relatum(
    root: &Path,
    base: &[String],
    note: &Note,
    author: &str,
    kind: &str,
) -> Result<(String, PathBuf), String> {
    if !root.is_dir() {
        return Err("no .swarmnotes/ found. Run `relatum init`, or set SWARMNOTES_DIR".into());
    }
    let bytes = render_relatum(note, author, kind).into_bytes();
    for n in 1..1000u32 {
        let segs = suffixed(base, n);
        let rel_id = segs.join("/");
        let rel = fsx::join_child(root, &segs.iter().map(String::as_str).collect::<Vec<_>>())?;
        let path = rel.with_extension("md");
        let parent = path
            .parent()
            .ok_or_else(|| format!("{} has no parent folder", path.display()))?;
        ensure_dir(parent)?;
        match fsx::write_atomic(&path, &bytes, false) {
            Ok(()) => return Ok((rel_id, path)),
            Err(e) if e.contains("already exists") => continue,
            Err(e) => return Err(format!("Relatum note \"{rel_id}\" not written: {e}")),
        }
    }
    Err("could not find a free Relatum id".into())
}

fn suffixed(base: &[String], n: u32) -> Vec<String> {
    let mut segs = base.to_vec();
    if n > 1 {
        if let Some(last) = segs.last_mut() {
            last.push_str(&format!("-{n}"));
        }
    }
    segs
}

fn ensure_dir(dir: &Path) -> Result<(), String> {
    match fs::symlink_metadata(dir) {
        Ok(m) if m.file_type().is_symlink() => Err(format!(
            "{} is a symlink; refusing to follow it",
            dir.display()
        )),
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err(format!("{} is not a directory", dir.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => fsx::make_private_dir(dir),
        Err(e) => Err(format!("cannot create {}: {e}", dir.display())),
    }
}

fn render_relatum(note: &Note, author: &str, kind: &str) -> String {
    let mut s = String::from("---\n");
    s.push_str(&format!(
        "kind: {kind}\nsummary: {}\nauthor: {author}\nupdated: {}\n",
        note.summary,
        fsx::today()
    ));
    if !note.tags.is_empty() {
        s.push_str(&format!("tags: [{}]\n", note.tags.join(", ")));
    }
    s.push_str("---\n\n");
    let mut body = if note.body.trim().is_empty() {
        note.summary.clone()
    } else {
        note.body.clone()
    };
    if !note.evidence.is_empty() {
        body.push_str("\n\nEvidence: ");
        body.push_str(&note.evidence.join(", "));
    }
    s.push_str(body.trim_end_matches('\n'));
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_match_relatum() {
        assert!(valid_relatum_segment("_shared"));
        assert!(valid_relatum_segment("net"));
        assert!(valid_relatum_segment("a-b"));
        assert!(!valid_relatum_segment(""));
        assert!(!valid_relatum_segment("_"));
        assert!(!valid_relatum_segment("__a"));
        assert!(!valid_relatum_segment("Net"));
        assert!(!valid_relatum_segment("-a"));
        assert!(!valid_relatum_segment(".."));
        assert!(reject_escape("../x").is_err());
        assert!(reject_escape("/etc/passwd").is_err());
        assert!(reject_escape("a/../../b").is_err());
        assert!(id_segments("a/./b", "pick").is_err());
        assert!(id_segments("_shared/net", "retry/post").is_ok());
        assert!(reject_escape("_shared/net").is_ok());
    }

    #[test]
    fn questions_wait_until_resolved() {
        let mut note = Note {
            id: "q".into(),
            kind: "question".into(),
            status: "open".into(),
            summary: "s".into(),
            agent: "main".into(),
            created: "t".into(),
            evidence: vec![],
            tags: vec![],
            promoted: None,
            body: String::new(),
        };
        assert!(relatum_kind(&note).is_err());
        note.status = "resolved".into();
        assert_eq!(relatum_kind(&note).unwrap(), "finding");
        note.kind = "dead-end".into();
        assert_eq!(relatum_kind(&note).unwrap(), "attempt");
    }
}
