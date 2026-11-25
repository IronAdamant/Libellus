//! Regression tests for the third verification pass.
mod common;
use common::*;
use std::fs::{self, File};
use std::io::Write;
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime};

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .output()
        .unwrap()
}

fn note(dir: &std::path::Path, id: &str, created: &str) {
    let path = dir.join(format!(".libellus/agents/main/notes/{id}.md"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        path,
        format!(
            "---\nkind: finding\nstatus: resolved\nsummary: A note\nagent: main\ncreated: {created}\n---\n\nBody.\n"
        ),
    )
    .unwrap();
}

#[test]
#[cfg(unix)]
fn a_fifo_is_not_opened_and_a_symlink_ledger_is_refused() {
    let dir = project("v3-fifo");
    init(&dir);
    let notes = dir.join(".libellus/agents/main/notes");
    fs::create_dir_all(&notes).unwrap();
    let fifo = notes.join("pipe.md");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let started = std::time::Instant::now();
    let listed = cli(&dir, &["ls", "--agent", "main"]);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "ls hung on a fifo"
    );
    let body = String::from_utf8_lossy(&listed.stdout);
    let err = String::from_utf8_lossy(&listed.stderr);
    assert!(listed.status.success(), "{err}");
    assert!(
        body.contains("not a regular file") || err.contains("not a regular file"),
        "{body} {err}"
    );
    let checked = cli(&dir, &["check"]);
    let check_err = String::from_utf8_lossy(&checked.stderr);
    assert!(!checked.status.success(), "{check_err}");
    assert!(check_err.contains("not a regular file"), "{check_err}");

    let ledger = dir.join(".libellus/agents/main/ledger.jsonl");
    let _ = fs::remove_file(&fifo);
    let _ = fs::remove_file(&ledger);
    assert!(Command::new("mkfifo")
        .arg(&ledger)
        .status()
        .unwrap()
        .success());
    let started = std::time::Instant::now();
    let spent = cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "retry",
            "--tokens_in",
            "1",
        ],
    );
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "spend hung on a fifo"
    );
    let spend_err = String::from_utf8_lossy(&spent.stderr);
    assert!(!spent.status.success(), "{spend_err}");
    assert!(spend_err.contains("not a regular file"), "{spend_err}");

    let _ = fs::remove_file(&ledger);
    let outside = dir.join("outside.jsonl");
    fs::write(&outside, "untouched\n").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, &ledger).unwrap();
        let spent = cli(
            &dir,
            &[
                "spend",
                "--agent",
                "main",
                "--task",
                "retry",
                "--tokens_in",
                "1",
            ],
        );
        let err = String::from_utf8_lossy(&spent.stderr);
        assert!(!spent.status.success(), "{err}");
        assert!(err.contains("symlink"), "{err}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "untouched\n");
        let read = cli(&dir, &["ledger", "--agent", "main"]);
        let err = String::from_utf8_lossy(&read.stderr);
        assert!(err.contains("symlink"), "{err}");
    }
}

#[test]
fn bad_note_names_dates_and_ledger_lines_are_flagged() {
    let dir = project("v3-names");
    init(&dir);
    note(&dir, "ok-note", "2026-11-25T00:00:00Z");
    note(&dir, "Bad_Name", "2026-11-25T00:00:00Z");
    let deep = dir.join(".libellus/agents/main/notes/a/b");
    fs::create_dir_all(&deep).unwrap();
    fs::write(
        deep.join("c.md"),
        "---\nkind: finding\nstatus: resolved\nsummary: A note\nagent: main\ncreated: 2026-11-25T00:00:00Z\n---\n\nBody.\n",
    )
    .unwrap();
    let listed = cli(&dir, &["ls", "--agent", "main"]);
    let body = String::from_utf8(listed.stdout).unwrap();
    assert!(listed.status.success(), "{body}");
    assert!(body.contains("\"id\":\"ok-note\""), "{body}");
    assert!(!body.contains("\"id\":\"Bad_Name\""), "{body}");
    assert!(!body.contains("\"id\":\"a/b/c\""), "{body}");
    assert!(body.contains("not an id"), "{body}");
    let checked = cli(&dir, &["check"]);
    let err = String::from_utf8_lossy(&checked.stderr);
    assert!(!checked.status.success(), "{err}");
    assert!(err.contains("Bad_Name") && err.contains("a/b/c"), "{err}");

    note(&dir, "bad-date", "yesterday");
    let checked = cli(&dir, &["check"]);
    let err = String::from_utf8_lossy(&checked.stderr);
    assert!(
        err.contains("created") && err.contains("yesterday"),
        "{err}"
    );
    let _ = fs::remove_file(dir.join(".libellus/agents/main/notes/bad-date.md"));

    let spent = cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "retry",
            "--tokens_in",
            "3",
        ],
    );
    assert!(
        spent.status.success(),
        "{}",
        String::from_utf8_lossy(&spent.stderr)
    );
    let ledger = dir.join(".libellus/agents/main/ledger.jsonl");
    let mut file = fs::OpenOptions::new().append(true).open(&ledger).unwrap();
    writeln!(
        file,
        r#"{{"ts":"2026-11-25T00:00:01Z","agent":"other","task":"retry","tokens_in":9,"tokens_out":0,"cost_micro_usd":0}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"ts":"not-a-time","agent":"main","task":"retry","tokens_in":8,"tokens_out":0,"cost_micro_usd":0}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"ts":"2026-11-25T00:00:02Z","agent":"main","task":"Bad","tokens_in":7,"tokens_out":0,"cost_micro_usd":0}}"#
    )
    .unwrap();
    drop(file);
    let report = cli(&dir, &["ledger", "--agent", "main"]);
    let body = String::from_utf8(report.stdout).unwrap();
    assert!(report.status.success(), "{body}");
    assert!(body.contains("\"entries\":1"), "{body}");
    assert!(
        !body.contains("\"tokens_in\":27") && !body.contains("\"tokens_in\":19"),
        "{body}"
    );
    let checked = cli(&dir, &["check"]);
    let err = String::from_utf8_lossy(&checked.stderr);
    assert!(err.contains("does not match"), "{err}");
    assert!(err.contains("not YYYY-MM-DDTHH:MM:SSZ"), "{err}");
    assert!(err.contains("not a slug"), "{err}");
}

#[test]
fn a_torn_ledger_line_is_ignored_on_read_and_blocks_spend() {
    let dir = project("v3-torn");
    init(&dir);
    assert!(cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "retry",
            "--tokens_in",
            "1"
        ],
    )
    .status
    .success());
    let ledger = dir.join(".libellus/agents/main/ledger.jsonl");
    let mut text = fs::read_to_string(&ledger).unwrap();
    text.push_str("{\"ts\":\"torn\"");
    fs::write(&ledger, &text).unwrap();
    let brief = cli(&dir, &["brief", "--agent", "main"]);
    let body = String::from_utf8(brief.stdout).unwrap();
    assert!(brief.status.success(), "{body}");
    assert!(body.contains("partial line"), "{body}");
    assert!(
        body.contains("\"entries\":1") || body.contains("(1 ledger entry)"),
        "{body}"
    );
    let before = fs::read_to_string(&ledger).unwrap();
    let spent = cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "retry",
            "--tokens_in",
            "1",
        ],
    );
    let err = String::from_utf8_lossy(&spent.stderr);
    assert!(!spent.status.success(), "{err}");
    assert!(
        err.contains("not appended") && err.contains("partial line"),
        "{err}"
    );
    assert_eq!(fs::read_to_string(&ledger).unwrap(), before);
    let checked = cli(&dir, &["check"]);
    let err = String::from_utf8_lossy(&checked.stderr);
    assert!(
        err.contains("partial line") && err.contains("delete"),
        "{err}"
    );
}

#[test]
fn ledger_line_and_file_caps_are_errors() {
    let dir = project("v3-cap");
    init(&dir);
    fs::create_dir_all(dir.join(".libellus/agents/main")).unwrap();
    let ledger = dir.join(".libellus/agents/main/ledger.jsonl");
    let mut big = vec![b'x'; 64 * 1024 + 1];
    big.push(b'\n');
    fs::write(&ledger, &big).unwrap();
    let read = cli(&dir, &["ledger", "--agent", "main"]);
    let err = String::from_utf8_lossy(&read.stderr);
    assert!(!read.status.success(), "{err}");
    assert!(err.contains("byte line cap"), "{err}");

    let file = File::create(&ledger).unwrap();
    file.set_len(32 * 1024 * 1024 + 1).unwrap();
    drop(file);
    let read = cli(&dir, &["ledger", "--agent", "main"]);
    let err = String::from_utf8_lossy(&read.stderr);
    assert!(
        err.contains("byte cap") && !err.contains("line cap"),
        "{err}"
    );
}

#[test]
fn generated_ids_survive_a_race_and_flags_accept_equals_and_a_separator() {
    let dir = project("v3-ids");
    init(&dir);
    let mut handles = Vec::new();
    for _ in 0..20 {
        let dir = dir.clone();
        handles.push(thread::spawn(move || {
            Command::new(CLI)
                .args([
                    "note",
                    "--agent",
                    "main",
                    "--kind",
                    "finding",
                    "--summary",
                    "Same summary",
                ])
                .current_dir(&dir)
                .env_remove("LIBELLUS_DIR")
                .output()
                .unwrap()
        }));
    }
    let mut ids = Vec::new();
    for handle in handles {
        let out = handle.join().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{err}");
        let body = String::from_utf8(out.stdout).unwrap();
        assert!(!err.contains("pass a new id"), "{err}");
        ids.push(body);
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 20, "{ids:?}");

    let eq = cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--summary=--starts-with-dashes",
            "--id",
            "dashed",
        ],
    );
    assert!(
        eq.status.success(),
        "{}",
        String::from_utf8_lossy(&eq.stderr)
    );
    let got = cli(&dir, &["get", "--agent", "main", "--id", "dashed"]);
    let body = String::from_utf8(got.stdout).unwrap();
    assert!(body.contains("--starts-with-dashes"), "{body}");
    let sep = cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--id",
            "dashed-2",
            "--summary",
            "--",
            "--also",
        ],
    );
    assert!(
        sep.status.success(),
        "{}",
        String::from_utf8_lossy(&sep.stderr)
    );
    let got = cli(&dir, &["get", "--agent", "main", "--id", "dashed-2"]);
    let body = String::from_utf8(got.stdout).unwrap();
    assert!(body.contains("--also"), "{body}");
}

#[test]
fn limit_errors_name_the_real_range_and_jsonrpc_is_required() {
    let dir = project("v3-limit");
    init(&dir);
    let line = call("libellus_ls", r#"{"agent":"main","limit":-1}"#, 1);
    let replies = talk(&dir, line.as_bytes());
    assert!(
        replies[0].contains("from 1 up to") && !replies[0].contains("from 0 up to"),
        "{}",
        replies[0]
    );
    let zero = call("libellus_ls", r#"{"agent":"main","limit":0}"#, 2);
    let replies = talk(&dir, zero.as_bytes());
    assert!(
        replies[0].contains("limit is 0") && replies[0].contains("from 1 up to"),
        "{}",
        replies[0]
    );
    let bad = talk(&dir, b"{\"id\":1,\"method\":\"ping\"}\n");
    assert!(bad[0].contains("jsonrpc must be"), "{}", bad[0]);
}

#[test]
fn a_bare_libellus_dir_needs_init_and_dirs_are_private() {
    let dir = project("v3-bare");
    let bare = dir.join("bare");
    fs::create_dir_all(&bare).unwrap();
    let out = Command::new(CLI)
        .args(["ls", "--agent", "main"])
        .current_dir(&dir)
        .env("LIBELLUS_DIR", &bare)
        .env_remove("LIBELLUS_AGENT")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("libellus init"), "{err}");

    init(&dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &std::path::Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir.join(".libellus")), 0o700);
        assert_eq!(mode(&dir.join(".libellus/agents")), 0o700);
    }
}

#[test]
fn check_reports_a_stale_lock_and_how_to_clear_it() {
    let dir = project("v3-lock");
    init(&dir);
    fs::create_dir_all(dir.join(".libellus/agents/main")).unwrap();
    let lock = dir.join(".libellus/agents/main/.ledger.jsonl.lock");
    fs::write(&lock, "pid 999999\n").unwrap();
    let file = File::options().write(true).open(&lock).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(120))
        .unwrap();
    drop(file);
    let checked = cli(&dir, &["check"]);
    let err = String::from_utf8_lossy(&checked.stderr);
    let body = String::from_utf8(checked.stdout).unwrap();
    assert!(checked.status.success(), "{err} {body}");
    assert!(
        body.contains("removed") && body.contains(".ledger.jsonl.lock"),
        "{body}"
    );
    assert!(!lock.exists(), "stale lock should have been removed");
}
