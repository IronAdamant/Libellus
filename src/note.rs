//! A note: a short markdown file with flat frontmatter, in the same family as
//! Relatum notes (`kind`, `summary`, `author`, `tags`) plus `status`,
//! `evidence` pointers and `promoted`. Parsing is strict: an unknown key, a
//! duplicate key, a bad value or a missing field is an error naming the file.
//! A damaged note is never skipped or treated as absent.

use crate::fsx;
use crate::store::Store;
use std::fs;
use std::path::Path;

pub const KINDS: &[&str] = &["decision", "dead-end", "question", "finding", "context"];
pub const STATUSES: &[&str] = &["open", "resolved"];
pub const MAX_SUMMARY: usize = 200;

#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub summary: String,
    pub agent: String,
    pub created: String,
    pub evidence: Vec<String>,
    pub tags: Vec<String>,
    pub promoted: Option<String>,
    pub body: String,
}

/// Evidence pointers name where proof lives instead of copying it:
/// `<tool>:<reference>`, for example `trammel:plan-3/step-2`.
pub fn valid_pointer(p: &str) -> bool {
    match p.split_once(':') {
        Some((tool, r)) => {
            fsx::valid_slug(tool)
                && !r.is_empty()
                && r.chars().count() <= 200
                && !r
                    .chars()
                    .any(|c| c.is_whitespace() || c.is_control() || matches!(c, ',' | '[' | ']'))
        }
        None => false,
    }
}

pub fn validate_summary(s: &str) -> Result<(), String> {
    validate_line("summary", s, true)
}

/// A one-line outcome appended by `libellus_resolve`. Same limits as a summary.
pub fn validate_outcome(s: &str) -> Result<(), String> {
    validate_line("outcome", s, false)
}

fn validate_line(label: &str, s: &str, suggest_body: bool) -> Result<(), String> {
    if s.trim().is_empty() {
        return Err(format!("{label} is empty"));
    }
    if s.contains('\n') || s.contains('\r') {
        return Err(format!("{label} must be one line"));
    }
    if s.chars().count() > MAX_SUMMARY {
        let extra = if suggest_body {
            "; put the detail in the body"
        } else {
            ""
        };
        return Err(format!(
            "{label} is longer than {MAX_SUMMARY} characters{extra}"
        ));
    }
    if s != s.trim() {
        return Err(format!("{label} has leading or trailing spaces"));
    }
    Ok(())
}

/// Append one outcome line. The previous body is kept, and a second call is
/// the caller's job to refuse: this only builds the text.
pub fn append_outcome(body: &str, outcome: &str) -> String {
    let line = format!("Outcome: {outcome}");
    if body.is_empty() {
        line
    } else {
        format!("{body}\n\n{line}")
    }
}

/// Path of one note. The id is checked; the file need not exist yet.
pub fn note_path(store: &Store, agent: &str, id: &str) -> Result<std::path::PathBuf, String> {
    Ok(fsx::join_id(&store.notes_dir(agent)?, id, 2)?.with_extension("md"))
}

/// One note. Missing, corrupt, a symlink, or a file whose `agent` field is
/// not `agent` is an error. Never an empty success.
pub fn load_one(store: &Store, agent: &str, id: &str) -> Result<Note, String> {
    let path = note_path(store, agent, id)?;
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("no note \"{id}\" for agent \"{agent}\""));
        }
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    if meta.file_type().is_symlink() {
        return Err(format!(
            "{} is a symlink; refusing to follow it",
            path.display()
        ));
    }
    if !meta.is_file() {
        return Err(format!("{} is not a note file", path.display()));
    }
    let text = fsx::read_strict(&path)?;
    let note = Note::parse(id, &text)
        .map_err(|e| format!("corrupt note {id} ({}): {e}", path.display()))?;
    if note.agent != agent {
        return Err(format!(
            "note \"{id}\" belongs to agent \"{}\", not \"{agent}\"",
            note.agent
        ));
    }
    Ok(note)
}

impl Note {
    pub fn render(&self) -> String {
        let mut s = String::from("---\n");
        s.push_str(&format!(
            "kind: {}\nstatus: {}\nsummary: {}\nagent: {}\ncreated: {}\n",
            self.kind, self.status, self.summary, self.agent, self.created
        ));
        if !self.evidence.is_empty() {
            s.push_str(&format!("evidence: [{}]\n", self.evidence.join(", ")));
        }
        if !self.tags.is_empty() {
            s.push_str(&format!("tags: [{}]\n", self.tags.join(", ")));
        }
        if let Some(p) = &self.promoted {
            s.push_str(&format!("promoted: {p}\n"));
        }
        s.push_str("---\n");
        if !self.body.is_empty() {
            s.push('\n');
            s.push_str(self.body.trim_end_matches('\n'));
            s.push('\n');
        }
        s
    }

    pub fn parse(id: &str, text: &str) -> Result<Note, String> {
        let mut lines = text.split('\n');
        if lines.next() != Some("---") {
            return Err("does not start with a --- frontmatter line".into());
        }
        let mut fields: Vec<(String, String)> = Vec::new();
        let mut closed = false;
        let mut consumed = 4; // "---\n"
        for line in lines.by_ref() {
            consumed += line.len() + 1;
            if line == "---" {
                closed = true;
                break;
            }
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let (k, v) = line
                .split_once(':')
                .ok_or_else(|| format!("bad frontmatter line \"{}\"", fsx::clip(line, 60)))?;
            let (k, v) = (k.trim(), v.trim());
            if ![
                "kind", "status", "summary", "agent", "created", "evidence", "tags", "promoted",
            ]
            .contains(&k)
            {
                return Err(format!("unknown frontmatter key \"{k}\""));
            }
            if fields.iter().any(|(fk, _)| fk == k) {
                return Err(format!("duplicate frontmatter key \"{k}\""));
            }
            fields.push((k.to_string(), v.to_string()));
        }
        if !closed {
            return Err("frontmatter is never closed with ---".into());
        }
        let get = |k: &str| {
            fields
                .iter()
                .find(|(fk, _)| fk == k)
                .map(|(_, v)| v.clone())
        };
        let need = |k: &str| {
            get(k)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("missing frontmatter field \"{k}\""))
        };
        let kind = need("kind")?;
        if !KINDS.contains(&kind.as_str()) {
            return Err(format!(
                "kind \"{kind}\" is not one of: {}",
                KINDS.join(", ")
            ));
        }
        let status = need("status")?;
        if !STATUSES.contains(&status.as_str()) {
            return Err(format!(
                "status \"{status}\" is not one of: {}",
                STATUSES.join(", ")
            ));
        }
        let summary = need("summary")?;
        validate_summary(&summary)?;
        let list = |k: &str| -> Result<Vec<String>, String> {
            match get(k) {
                None => Ok(Vec::new()),
                Some(v) => {
                    let inner = v
                        .strip_prefix('[')
                        .and_then(|r| r.strip_suffix(']'))
                        .ok_or_else(|| format!("{k} must be written as [a, b]"))?;
                    Ok(inner
                        .split(',')
                        .map(str::trim)
                        .filter(|x| !x.is_empty())
                        .map(String::from)
                        .collect())
                }
            }
        };
        let evidence = list("evidence")?;
        if let Some(bad) = evidence.iter().find(|p| !valid_pointer(p)) {
            return Err(format!(
                "bad evidence pointer \"{bad}\" (use tool:reference)"
            ));
        }
        let tags = list("tags")?;
        if let Some(bad) = tags.iter().find(|t| !fsx::valid_slug(t)) {
            return Err(format!("bad tag \"{bad}\""));
        }
        let body_start = consumed.min(text.len());
        let body = text[body_start..].trim_matches('\n').to_string();
        Ok(Note {
            id: id.to_string(),
            kind,
            status,
            summary,
            agent: need("agent")?,
            created: need("created")?,
            evidence,
            tags,
            promoted: get("promoted").filter(|p| !p.is_empty()),
            body,
        })
    }
}

/// Every note of an agent, oldest first. A damaged note fails the whole read.
pub fn load_all(store: &Store, agent: &str) -> Result<Vec<Note>, String> {
    let dir = store.notes_dir(agent)?;
    let mut ids = Vec::new();
    collect(&dir, &dir, &mut ids)?;
    let mut notes = if ids.len() < 128 {
        read_notes(&ids)?
    } else {
        read_notes_parallel(&ids)?
    };
    notes.sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
    Ok(notes)
}

fn read_one(id: &str, path: &Path) -> Result<Note, String> {
    let text = fsx::read_strict(path)?;
    Note::parse(id, &text).map_err(|e| format!("corrupt note {id} ({}): {e}", path.display()))
}

fn read_notes(ids: &[(String, std::path::PathBuf)]) -> Result<Vec<Note>, String> {
    let mut notes = Vec::with_capacity(ids.len());
    for (id, path) in ids {
        notes.push(read_one(id, path)?);
    }
    Ok(notes)
}

/// Read many small note files at once. The first error in directory order is
/// the one returned, same as a single-threaded walk.
fn read_notes_parallel(ids: &[(String, std::path::PathBuf)]) -> Result<Vec<Note>, String> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(2, 8);
    let chunk = ids.len().div_ceil(workers).max(1);
    let mut slots: Vec<Option<Result<Note, String>>> = (0..ids.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        for (chunk_ids, chunk_slots) in ids.chunks(chunk).zip(slots.chunks_mut(chunk)) {
            scope.spawn(move || {
                for ((id, path), slot) in chunk_ids.iter().zip(chunk_slots.iter_mut()) {
                    *slot = Some(read_one(id, path));
                }
            });
        }
    });
    let mut notes = Vec::with_capacity(ids.len());
    for slot in slots {
        notes.push(slot.ok_or_else(|| "internal error: a note was not read".to_string())??);
    }
    Ok(notes)
}

fn collect(
    base: &Path,
    dir: &Path,
    out: &mut Vec<(String, std::path::PathBuf)>,
) -> Result<(), String> {
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("cannot read {}: {e}", dir.display())),
    };
    for e in rd {
        let e = e.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue; // temp files from a write in flight
        }
        let ft = e
            .file_type()
            .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;
        if ft.is_symlink() {
            return Err(format!(
                "{} is a symlink; refusing to follow it",
                path.display()
            ));
        }
        if ft.is_dir() {
            collect(base, &path, out)?;
        } else if let Some(stem) = name.strip_suffix(".md") {
            let rel = path
                .parent()
                .unwrap_or(base)
                .strip_prefix(base)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let id = if rel.is_empty() {
                stem.to_string()
            } else {
                format!("{rel}/{stem}")
            };
            out.push((id, path));
        } else {
            return Err(format!(
                "unexpected file {} in the notes folder",
                path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Note {
        Note {
            id: "x".into(),
            kind: "question".into(),
            status: "open".into(),
            summary: "Does retry apply to POST?".into(),
            agent: "main".into(),
            created: "2026-11-25T00:00:00Z".into(),
            evidence: vec!["trammel:plan-3/step-2".into()],
            tags: vec!["net".into()],
            promoted: None,
            body: "Line one.\n\n---\nstill body".into(),
        }
    }

    #[test]
    fn round_trip() {
        let n = sample();
        assert_eq!(Note::parse("x", &n.render()).unwrap(), n);
    }

    #[test]
    fn strictness() {
        let ok = sample().render();
        assert!(Note::parse("x", &ok.replace("kind: question", "kind: rumour")).is_err());
        assert!(Note::parse(
            "x",
            &ok.replace("status: open", "status: open\nstatus: resolved")
        )
        .is_err());
        assert!(Note::parse("x", &ok.replace("agent: main", "agent: main\ncolour: red")).is_err());
        assert!(Note::parse("x", &ok.replace("trammel:plan-3/step-2", "no pointer")).is_err());
        assert!(Note::parse("x", "no frontmatter").is_err());
        assert!(Note::parse("x", "---\nkind: question\n").is_err());
    }

    #[test]
    fn pointers() {
        assert!(valid_pointer("chisel:tests/a.rs"));
        assert!(!valid_pointer("chisel:"));
        assert!(!valid_pointer("chisel:a b"));
        assert!(!valid_pointer("Chisel:a"));
    }

    #[test]
    fn outcome_rules_and_append() {
        assert!(validate_outcome("No, GET only.").is_ok());
        assert!(validate_outcome("").is_err());
        assert!(validate_outcome("a\nb").is_err());
        assert!(validate_outcome(&"x".repeat(MAX_SUMMARY + 1)).is_err());
        assert!(validate_outcome(&"é".repeat(MAX_SUMMARY)).is_ok());
        assert_eq!(
            append_outcome("Line one.", "No, GET only."),
            "Line one.\n\nOutcome: No, GET only."
        );
        assert_eq!(append_outcome("", "Done."), "Outcome: Done.");
    }
}
