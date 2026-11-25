//! Promoting a private note writes a Relatum file and does not run Relatum.
mod common;
use common::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cli(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .env_remove("SWARMNOTES_DIR")
        .output()
        .unwrap()
}

fn cli_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(CLI);
    cmd.args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .env_remove("SWARMNOTES_DIR");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn note(dir: &Path, id: &str, kind: &str, status: Option<&str>, body: &str) {
    let mut args = vec![
        "note",
        "--agent",
        "main",
        "--kind",
        kind,
        "--id",
        id,
        "--summary",
        "Keep the summary",
        "--body",
        body,
        "--evidence",
        "trammel:plan-3/step-2",
        "--tags",
        "net",
    ];
    if let Some(status) = status {
        args.extend(["--status", status]);
    }
    let out = cli(dir, &args);
    assert!(
        out.status.success(),
        "{}: {}",
        id,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn field(text: &str, key: &str) -> String {
    let head = text.split("\n---\n").next().unwrap_or("");
    head.lines()
        .find_map(|line| line.strip_prefix(&format!("{key}: ")))
        .unwrap_or("")
        .to_string()
}

fn markdowns(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
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
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                out.push(path);
            }
        }
    }
    walk(dir, &mut out);
    out.sort();
    out
}

#[test]
fn promoting_a_resolved_decision_writes_relatum_and_records_the_id() {
    let dir = project("promote-ok");
    init(&dir);
    fs::create_dir(dir.join(".swarmnotes")).unwrap();
    note(
        &dir,
        "retry/post",
        "decision",
        None,
        "Retry GET only.\n\nWhy: POST is not idempotent.",
    );
    let out = cli(
        &dir,
        &[
            "promote",
            "--agent",
            "main",
            "--id",
            "retry/post",
            "--scope",
            "net",
        ],
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{} {}",
        stdout,
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("\"promoted\":\"net/retry/post\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"relatum_kind\":\"decision\""), "{stdout}");
    let rel = fs::read_to_string(dir.join(".swarmnotes/net/retry/post.md")).unwrap();
    assert_eq!(field(&rel, "kind"), "decision");
    assert_eq!(field(&rel, "summary"), "Keep the summary");
    assert_eq!(field(&rel, "author"), "main");
    assert_eq!(field(&rel, "tags"), "[net]");
    let updated = field(&rel, "updated");
    assert!(
        updated.len() == 10 && updated.as_bytes()[4] == b'-' && updated.as_bytes()[7] == b'-',
        "{updated}"
    );
    assert!(rel.contains("Retry GET only."));
    assert!(rel.contains("Evidence: trammel:plan-3/step-2"), "{rel}");
    assert!(rel.starts_with("---\nkind: decision\n"));
    let private =
        fs::read_to_string(dir.join(".libellus/agents/main/notes/retry/post.md")).unwrap();
    assert!(private.contains("promoted: net/retry/post"), "{private}");
    assert!(private.contains("created:"));
    assert_eq!(markdowns(&dir.join(".swarmnotes")).len(), 1);
    assert!(strays(&dir).is_empty(), "{:?}", strays(&dir));

    let again = cli(
        &dir,
        &[
            "promote",
            "--agent",
            "main",
            "--id",
            "retry/post",
            "--scope",
            "net",
        ],
    );
    assert_eq!(again.status.code(), Some(1));
    let err = String::from_utf8_lossy(&again.stderr);
    assert!(
        err.contains("already promoted") && err.contains("net/retry/post"),
        "{err}"
    );
    fs::remove_file(dir.join(".swarmnotes/net/retry/post.md")).unwrap();
    let third = cli(
        &dir,
        &[
            "promote",
            "--agent",
            "main",
            "--id",
            "retry/post",
            "--scope",
            "net",
        ],
    );
    assert_eq!(third.status.code(), Some(1));
    assert!(markdowns(&dir.join(".swarmnotes")).is_empty());
}

#[test]
fn kind_mapping_and_an_open_question_is_refused() {
    let dir = project("promote-kinds");
    init(&dir);
    fs::create_dir(dir.join(".swarmnotes")).unwrap();
    let cases = [
        ("d", "decision", Some("open"), "decision", true),
        ("c", "context", None, "context", true),
        ("f", "finding", None, "finding", true),
        ("e", "dead-end", None, "attempt", true),
        ("q", "question", Some("resolved"), "finding", true),
        ("o", "question", Some("open"), "", false),
    ];
    for (id, kind, status, expect, ok) in cases {
        note(&dir, id, kind, status, "body");
        let out = cli(
            &dir,
            &[
                "promote", "--agent", "main", "--id", id, "--scope", "_shared",
            ],
        );
        if ok {
            assert!(
                out.status.success(),
                "{id}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let text =
                fs::read_to_string(dir.join(format!(".swarmnotes/_shared/{id}.md"))).unwrap();
            assert_eq!(field(&text, "kind"), expect, "{id}");
        } else {
            assert_eq!(out.status.code(), Some(1), "{id}");
            assert!(
                String::from_utf8_lossy(&out.stderr).contains("open question"),
                "{id}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(!dir.join(format!(".swarmnotes/_shared/{id}.md")).exists());
        }
    }
}

#[test]
fn a_missing_store_creates_nothing_and_bad_scopes_stay_inside() {
    let dir = project("promote-none");
    init(&dir);
    note(&dir, "pick", "decision", None, "body");
    let out = cli(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "pick", "--scope", "net",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("relatum init"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!dir.join(".swarmnotes").exists());
    let private = fs::read_to_string(dir.join(".libellus/agents/main/notes/pick.md")).unwrap();
    assert!(!private.contains("promoted:"));

    fs::create_dir(dir.join(".swarmnotes")).unwrap();
    for scope in ["../out", "/etc/passwd", "a/../../b", "a/./b", "Bad", ""] {
        let out = cli(
            &dir,
            &[
                "promote", "--agent", "main", "--id", "pick", "--scope", scope,
            ],
        );
        assert_eq!(out.status.code(), Some(1), "{scope}");
        assert!(!private_has_promoted(&dir), "{scope} wrote a promotion");
    }
    assert!(markdowns(&dir.join(".swarmnotes")).is_empty());
    assert!(!dir.join("out").exists());
    assert!(!dir.join("b").exists());
    assert!(strays(&dir).is_empty());

    let missing = dir.join("missing-store");
    let out = cli_env(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "pick", "--scope", "net",
        ],
        &[("SWARMNOTES_DIR", missing.to_str().unwrap())],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(!missing.exists());

    let elsewhere = project("promote-elsewhere");
    fs::create_dir(elsewhere.join(".swarmnotes")).unwrap();
    let out = cli_env(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "pick", "--scope", "net",
        ],
        &[(
            "SWARMNOTES_DIR",
            elsewhere.join(".swarmnotes").to_str().unwrap(),
        )],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(elsewhere.join(".swarmnotes/net/pick.md").is_file());
    assert!(!dir.join(".swarmnotes/net").exists());
}

#[cfg(unix)]
#[test]
fn a_symlink_scope_is_refused_and_a_failed_update_leaves_one_relatum_file() {
    let dir = project("promote-escape");
    init(&dir);
    let outside = project("promote-escape-out");
    fs::create_dir_all(dir.join(".swarmnotes")).unwrap();
    std::os::unix::fs::symlink(&outside, dir.join(".swarmnotes/link")).unwrap();
    note(&dir, "pick", "finding", None, "secret body");
    let out = cli(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "pick", "--scope", "link",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("symlink"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(markdowns(&outside).is_empty());
    assert!(!private_has_promoted(&dir));

    let notes = dir.join(".libellus/agents/main/notes");
    let guard = ModeGuard(notes.clone());
    let mut perms = fs::metadata(&notes).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    perms.set_mode(0o500);
    fs::set_permissions(&notes, perms).unwrap();
    let out = cli(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "pick", "--scope", "net",
        ],
    );
    drop(guard);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        err.contains("Relatum note written") && err.contains("was not updated"),
        "{err}"
    );
    assert!(err.contains("pick.md"), "{err}");
    let files = markdowns(&dir.join(".swarmnotes"));
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(!private_has_promoted(&dir));
    assert!(strays(&dir).is_empty(), "{:?}", strays(&dir));
}

#[test]
fn a_taken_id_gets_a_suffix_and_unknown_arguments_fail() {
    let dir = project("promote-suffix");
    init(&dir);
    fs::create_dir_all(dir.join(".swarmnotes/net")).unwrap();
    fs::write(dir.join(".swarmnotes/net/pick.md"), "original\n").unwrap();
    note(&dir, "pick", "context", None, "");
    let out = cli(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "pick", "--scope", "net",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("\"promoted\":\"net/pick-2\""), "{stdout}");
    assert_eq!(
        fs::read_to_string(dir.join(".swarmnotes/net/pick.md")).unwrap(),
        "original\n"
    );
    let copy = fs::read_to_string(dir.join(".swarmnotes/net/pick-2.md")).unwrap();
    assert_eq!(field(&copy, "kind"), "context");
    assert!(copy.contains("Keep the summary"), "{copy}");
    assert!(strays(&dir).is_empty());

    let bad = cli(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "pick", "--scope", "net", "--colour", "red",
        ],
    );
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown flag"));
}

fn private_has_promoted(dir: &Path) -> bool {
    fs::read_to_string(dir.join(".libellus/agents/main/notes/pick.md"))
        .unwrap()
        .contains("promoted:")
}

#[cfg(unix)]
struct ModeGuard(PathBuf);

#[cfg(unix)]
impl Drop for ModeGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&self.0) {
            let mut perms = meta.permissions();
            perms.set_mode(0o700);
            let _ = fs::set_permissions(&self.0, perms);
        }
    }
}
