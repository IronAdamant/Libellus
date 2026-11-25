//! Regressions for the fourth stress test (Libellus 1.1.2 findings).
mod common;
use common::*;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .output()
        .unwrap()
}

fn must(dir: &std::path::Path, args: &[&str]) -> String {
    let out = cli(dir, args);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{args:?} {err}");
    String::from_utf8(out.stdout).unwrap()
}

fn section(text: &str, heading: &str) -> String {
    let rest = text.split(heading).nth(1).unwrap_or("");
    rest.split("\n## ").next().unwrap_or("").to_string()
}

#[test]
fn recent_decisions_follow_write_order_inside_one_second() {
    let dir = project("s4-order");
    init(&dir);
    let summaries = [
        "Use Postgres 16",
        "Keep the retry at three attempts",
        "Batch size is 500",
        "Ship behind a flag",
        "Drop the legacy cache",
        "Migrate orders before users",
        "Pin rustc 1.83",
        "Abort if downtime over 5 min",
    ];
    for summary in summaries {
        must(
            &dir,
            &[
                "note",
                "--agent",
                "main",
                "--kind",
                "decision",
                "--summary",
                summary,
            ],
        );
    }
    let text = must(&dir, &["brief", "--agent", "main"]);
    let recent = section(&text, "## Recent decisions");
    let ids = [
        "abort-if-downtime-over-5-min",
        "pin-rustc-1-83",
        "migrate-orders-before-users",
        "drop-the-legacy-cache",
        "ship-behind-a-flag",
    ];
    let mut at = 0usize;
    for id in ids {
        let found = recent[at..]
            .find(id)
            .unwrap_or_else(|| panic!("{id} missing in {recent}"));
        at += found + id.len();
    }
    assert!(!recent.contains("use-postgres-16"), "{recent}");
    assert!(
        !recent.contains("keep-the-retry-at-three-attempts"),
        "{recent}"
    );
    assert!(!recent.contains("batch-size-is-500"), "{recent}");
}

#[test]
fn a_decision_can_be_marked_replaced_and_the_brief_says_so() {
    let dir = project("s4-replaced");
    init(&dir);
    must(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "decision",
            "--id",
            "use-pg15",
            "--summary",
            "Use Postgres 15",
        ],
    );
    let reversed = cli(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "use-pg15",
            "--outcome",
            "REVERSED: use 16",
        ],
    );
    assert!(
        reversed.status.success(),
        "{}",
        String::from_utf8_lossy(&reversed.stderr)
    );
    must(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "decision",
            "--status",
            "open",
            "--id",
            "q-x",
            "--summary",
            "Use queue X",
        ],
    );
    must(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "q-x",
            "--outcome",
            "SUPERSEDED: use queue Y",
        ],
    );
    must(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "decision",
            "--id",
            "use-pg16",
            "--summary",
            "Use Postgres 16",
        ],
    );
    let plain = cli(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "use-pg16",
            "--outcome",
            "Still 16.",
        ],
    );
    let err = String::from_utf8_lossy(&plain.stderr);
    assert!(err.contains("already resolved"), "{err}");
    must(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "use-pg16",
            "--supersede",
            "--outcome",
            "Use 17 instead.",
        ],
    );
    let text = must(&dir, &["brief", "--agent", "main"]);
    let recent = section(&text, "## Recent decisions");
    assert!(!recent.contains("q-x"), "{recent}");
    assert!(!recent.contains("use-pg15"), "{recent}");
    let replaced = section(&text, "## Replaced decisions");
    assert!(
        replaced.contains("q-x: Use queue X (replaced)"),
        "{replaced}"
    );
    assert!(
        replaced.contains("use-pg15: Use Postgres 15 (replaced)"),
        "{replaced}"
    );
    assert!(
        replaced.contains("use-pg16: Use Postgres 16 (replaced)"),
        "{replaced}"
    );
}

#[test]
fn the_brief_caps_questions_keeps_the_newest_and_shows_state() {
    let dir = project("s4-brief-cap");
    init(&dir);
    let empty = must(&dir, &["brief", "--agent", "main"]);
    assert!(empty.contains("libellus_note --kind context"), "{empty}");
    for i in 0..8 {
        must(
            &dir,
            &[
                "note",
                "--agent",
                "main",
                "--kind",
                "question",
                "--id",
                &format!("q{i}"),
                "--summary",
                &format!("Question {i}"),
            ],
        );
    }
    must(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "context",
            "--id",
            "where",
            "--summary",
            "On the retry branch",
            "--body",
            "Next edit the client.",
        ],
    );
    must(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--id",
            "saw",
            "--summary",
            "POST is retried",
        ],
    );
    let text = must(&dir, &["brief", "--agent", "main", "--max_items", "3"]);
    let state = section(&text, "## Current state");
    assert!(state.contains("where: On the retry branch"), "{state}");
    assert!(state.contains("Next edit the client."), "{state}");
    let questions = section(&text, "## Open questions");
    assert!(questions.contains("showing 3"), "{questions}");
    assert!(questions.contains("omitted"), "{questions}");
    assert!(questions.contains("q7:"), "{questions}");
    assert!(questions.contains("q6:"), "{questions}");
    assert!(questions.contains("q5:"), "{questions}");
    assert!(!questions.contains("q0:"), "{questions}");
    assert!(questions.find("q7:").unwrap() < questions.find("q5:").unwrap());
    assert!(text.contains("saw: POST is retried"), "{text}");
    let reply = talk(
        &dir,
        call("libellus_brief", r#"{"agent":"main","max_items":3}"#, 1).as_bytes(),
    );
    assert!(is_ok(&reply[0]), "{}", reply[0]);
    assert!(
        reply[0].contains("open_questions_total\\\":8"),
        "{}",
        reply[0]
    );
    assert!(reply[0].contains("truncated\\\":true"), "{}", reply[0]);
}

#[test]
fn brief_text_stays_inside_its_byte_budget() {
    let dir = project("s4-budget");
    init(&dir);
    let summary = "s".repeat(180);
    for i in 0..50 {
        must(
            &dir,
            &[
                "note",
                "--agent",
                "main",
                "--kind",
                "question",
                "--id",
                &format!("q{i}"),
                "--summary",
                &summary,
            ],
        );
    }
    let text = must(&dir, &["brief", "--agent", "main", "--max_items", "50"]);
    assert!(text.len() <= 8192, "brief is {} bytes\n{text}", text.len());
    assert!(text.contains("Brief truncated at 8192 bytes"), "{text}");
}

#[test]
fn task_costs_keep_the_newest_task_and_grouping_scales() {
    let dir = project("s4-tasks");
    init(&dir);
    for task in [
        "alpha",
        "beta",
        "gamma",
        "delta",
        "epsilon",
        "zz-current-task",
    ] {
        must(
            &dir,
            &[
                "spend",
                "--agent",
                "main",
                "--task",
                task,
                "--tokens_in",
                "1",
            ],
        );
    }
    let text = must(&dir, &["brief", "--agent", "main", "--max_items", "3"]);
    let costs = section(&text, "## Costs");
    assert!(costs.contains("zz-current-task"), "{costs}");
    assert!(!costs.contains("- alpha:"), "{costs}");
    let reply = talk(
        &dir,
        call("libellus_brief", r#"{"agent":"main","max_items":3}"#, 1).as_bytes(),
    );
    assert!(reply[0].contains("by_task_total\\\":6"), "{}", reply[0]);
    assert!(reply[0].contains("by_task_omitted\\\":3"), "{}", reply[0]);
    assert!(reply[0].contains("zz-current-task"), "{}", reply[0]);

    let path = dir.join(".libellus/agents/main/ledger.jsonl");
    let mut file = File::create(&path).unwrap();
    for i in 0..2000 {
        writeln!(
            file,
            r#"{{"ts":"2026-11-20T00:00:00Z","agent":"main","task":"t{i}","tokens_in":1,"tokens_out":0,"cost_micro_usd":0}}"#
        )
        .unwrap();
    }
    drop(file);
    let wide = must(&dir, &["brief", "--agent", "main", "--max_items", "2"]);
    assert!(section(&wide, "## Costs").contains("- t1999:"), "{wide}");
    assert!(!section(&wide, "## Costs").contains("- t0:"), "{wide}");
    let reply = talk(
        &dir,
        call("libellus_brief", r#"{"agent":"main","max_items":2}"#, 2).as_bytes(),
    );
    assert!(reply[0].contains("by_task_total\\\":2000"), "{}", reply[0]);
    assert!(
        reply[0].contains("by_task_omitted\\\":1998"),
        "{}",
        reply[0]
    );
}

#[test]
fn spend_refuses_before_the_cap_and_the_brief_still_returns_notes() {
    let dir = project("s4-cap");
    init(&dir);
    must(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "question",
            "--id",
            "keep",
            "--summary",
            "must survive",
        ],
    );
    let ledger = dir.join(".libellus/agents/main/ledger.jsonl");
    let file = File::create(&ledger).unwrap();
    file.set_len(32 * 1024 * 1024 + 1).unwrap();
    drop(file);
    let spent = cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "long-task",
            "--tokens_in",
            "1",
        ],
    );
    let err = String::from_utf8_lossy(&spent.stderr);
    assert!(!spent.status.success(), "{err}");
    assert!(
        err.contains("byte cap") && err.contains("libellus archive"),
        "{err}"
    );
    assert_eq!(fs::metadata(&ledger).unwrap().len(), 32 * 1024 * 1024 + 1);
    let text = must(&dir, &["brief", "--agent", "main"]);
    assert!(text.contains("must survive"), "{text}");
    assert!(text.contains("Ledger:"), "{text}");
    assert!(text.contains("byte cap"), "{text}");
    assert!(text.contains("libellus archive"), "{text}");

    let near = project("s4-near");
    init(&near);
    must(
        &near,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "long-task",
            "--tokens_in",
            "1",
        ],
    );
    let ledger = near.join(".libellus/agents/main/ledger.jsonl");
    let file = OpenOptions::new().write(true).open(&ledger).unwrap();
    file.set_len(32 * 1024 * 1024 - 10).unwrap();
    drop(file);
    let before = fs::metadata(&ledger).unwrap().len();
    let spent = cli(
        &near,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "long-task",
            "--tokens_in",
            "1",
            "--note",
            &"x".repeat(100),
        ],
    );
    let err = String::from_utf8_lossy(&spent.stderr);
    assert!(!spent.status.success(), "{err}");
    assert!(
        err.contains("not appended") && err.contains("byte cap"),
        "{err}"
    );
    assert_eq!(fs::metadata(&ledger).unwrap().len(), before);
}

#[test]
fn archive_keeps_the_old_ledger_and_totals_start_again() {
    let dir = project("s4-archive");
    init(&dir);
    must(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "retry",
            "--tokens_in",
            "5",
        ],
    );
    let out = must(&dir, &["archive", "--agent", "main"]);
    assert!(out.contains("\"bytes\":"), "{out}");
    let archived = dir
        .join(".libellus/agents/main/archive")
        .read_dir()
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|ext| ext == "jsonl"))
        .unwrap();
    let old = fs::read_to_string(&archived).unwrap();
    assert!(old.contains("\"tokens_in\":5"), "{old}");
    assert!(!dir.join(".libellus/agents/main/ledger.jsonl").exists());
    let again = must(
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
    assert!(again.contains("\"entries\":1"), "{again}");
    let report = must(&dir, &["ledger", "--agent", "main"]);
    assert!(report.contains("\"tokens_in\":1"), "{report}");
    assert!(!report.contains("\"tokens_in\":6"), "{report}");
    must(&dir, &["check"]);
}

#[test]
fn a_long_ledger_line_and_a_fifo_do_not_blank_the_brief() {
    let dir = project("s4-line");
    init(&dir);
    must(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "question",
            "--id",
            "keep",
            "--summary",
            "must survive",
        ],
    );
    let ledger = dir.join(".libellus/agents/main/ledger.jsonl");
    let mut big = vec![b'x'; 64 * 1024 + 1];
    big.push(b'\n');
    fs::write(&ledger, &big).unwrap();
    let text = must(&dir, &["brief", "--agent", "main"]);
    assert!(
        text.contains("must survive") && text.contains("line cap"),
        "{text}"
    );
    let read = cli(&dir, &["ledger", "--agent", "main"]);
    assert!(!read.status.success());
    assert!(String::from_utf8_lossy(&read.stderr).contains("line cap"));

    #[cfg(unix)]
    {
        let dir = project("s4-fifo");
        init(&dir);
        must(
            &dir,
            &[
                "note",
                "--agent",
                "main",
                "--kind",
                "question",
                "--id",
                "keep",
                "--summary",
                "must survive",
            ],
        );
        let ledger = dir.join(".libellus/agents/main/ledger.jsonl");
        let _ = fs::remove_file(&ledger);
        let made = Command::new("mkfifo").arg(&ledger).status().unwrap();
        assert!(made.success());
        let dir2 = dir.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(cli(&dir2, &["brief", "--agent", "main"]));
        });
        let out = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("brief hung on a fifo ledger");
        let text = String::from_utf8(out.stdout).unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{err}");
        assert!(text.contains("must survive"), "{text}");
        assert!(text.contains("not a regular file"), "{text}");
    }
}

#[test]
fn a_bare_lock_and_a_dead_temp_are_recovered() {
    let dir = project("s4-lock");
    init(&dir);
    must(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "question",
            "--id",
            "q1",
            "--summary",
            "Locked?",
        ],
    );
    let lock = dir.join(".libellus/agents/main/notes/.q1.md.lock");
    fs::write(&lock, b"").unwrap();
    OpenOptions::new()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(5))
        .unwrap();
    let started = Instant::now();
    must(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "q1",
            "--outcome",
            "Cleared.",
        ],
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "bare lock blocked for {:?}",
        started.elapsed()
    );

    let dead = dir.join(".libellus/agents/main/.ledger.jsonl.tmp-999999-1");
    fs::write(&dead, b"leftover").unwrap();
    OpenOptions::new()
        .write(true)
        .open(&dead)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(120))
        .unwrap();
    let live = dir.join(format!(
        ".libellus/agents/main/.ledger.jsonl.tmp-{}-1",
        std::process::id()
    ));
    fs::write(&live, b"in progress").unwrap();
    let held = dir.join(".libellus/agents/main/notes/.q1.md.lock");
    fs::write(&held, b"held\n").unwrap();
    let body = must(&dir, &["check"]);
    assert!(!dead.exists(), "{body}");
    assert!(live.exists(), "{body}");
    assert!(held.exists(), "a fresh non-pid lock must stay: {body}");
    assert!(body.contains("999999"), "{body}");
}

#[cfg(unix)]
#[test]
fn spend_rolls_back_its_own_partial_append() {
    let dir = project("s4-rollback");
    init(&dir);
    must(
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
    let ledger = dir.join(".libellus/agents/main/ledger.jsonl");
    let before = fs::read(&ledger).unwrap();
    let model = "m".repeat(4000);
    let script = format!(
        "ulimit -f 1; exec {:?} spend --agent main --task retry --tokens_in 1 --model {}",
        CLI, model
    );
    let out = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .current_dir(&dir)
        .env_remove("LIBELLUS_DIR")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(err.contains("partial line was removed"), "{err}");
    assert_eq!(fs::read(&ledger).unwrap(), before, "{err}");
}

#[test]
fn later_spends_keep_the_right_total_and_a_full_read_repairs_the_cache() {
    let dir = project("s4-cache");
    init(&dir);
    let ledger = dir.join(".libellus/agents/main/ledger.jsonl");
    fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    let mut file = File::create(&ledger).unwrap();
    for _ in 0..400 {
        writeln!(
            file,
            r#"{{"ts":"2026-11-20T00:00:00Z","agent":"main","task":"retry","tokens_in":1,"tokens_out":0,"cost_micro_usd":0}}"#
        )
        .unwrap();
    }
    drop(file);
    let first = must(
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
    assert!(first.contains("\"entries\":401"), "{first}");
    let second = must(
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
    assert!(second.contains("\"entries\":402"), "{second}");
    assert!(dir.join(".libellus/agents/main/ledger.meta").is_file());
    let mut text = fs::read_to_string(&ledger).unwrap();
    text = text.replacen("\"tokens_in\":1", "\"tokens_in\":2", 1);
    fs::write(&ledger, &text).unwrap();
    let report = must(&dir, &["ledger", "--agent", "main"]);
    assert!(
        report.contains("\"tokens_in\":403"),
        "full read should see the edited line\n{report}"
    );
    let third = must(
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
    assert!(third.contains("\"tokens_in\":404"), "{third}");
}

#[test]
fn a_note_written_before_seq_still_reads() {
    let dir = project("s4-old");
    init(&dir);
    let notes = dir.join(".libellus/agents/main/notes");
    fs::create_dir_all(&notes).unwrap();
    fs::write(
        notes.join("old.md"),
        "---\nkind: decision\nstatus: resolved\nsummary: Old decision\nagent: main\ncreated: 2020-01-01T00:00:00Z\n---\n\n",
    )
    .unwrap();
    must(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "decision",
            "--id",
            "new",
            "--summary",
            "New decision",
        ],
    );
    let got = must(&dir, &["get", "--agent", "main", "--id", "old"]);
    assert!(got.contains("Old decision"), "{got}");
    assert!(!got.contains("\"seq\""), "{got}");
    let text = must(&dir, &["brief", "--agent", "main"]);
    let recent = section(&text, "## Recent decisions");
    assert!(recent.contains("new: New decision"), "{recent}");
    assert!(
        recent.find("new:").unwrap() < recent.find("old:").unwrap(),
        "{recent}"
    );
}
