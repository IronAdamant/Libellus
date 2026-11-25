//! Regression tests for the verifier findings on 1.0.0 and the 1.1.0 branch.
mod common;
use common::*;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .output()
        .unwrap()
}

#[test]
fn spend_does_not_append_when_the_total_would_overflow() {
    let dir = project("reg-over");
    init(&dir);
    let max = u64::MAX.to_string();
    let first = cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "retry",
            "--tokens_in",
            &max,
        ],
    );
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let second = cli(
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
    let err = String::from_utf8_lossy(&second.stderr);
    assert_eq!(second.status.code(), Some(1), "{err}");
    assert!(
        err.contains("not appended") && err.contains("overflow"),
        "{err}"
    );
    let path = dir.join(".libellus/agents/main/ledger.jsonl");
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 1, "{text}");

    // A ledger that already overflows must not brick brief or ledger.
    let mut extra = text.clone();
    extra.push_str(&text.replace(&max, "1"));
    fs::write(&path, &extra).unwrap();
    let ledger = cli(&dir, &["ledger", "--agent", "main"]);
    let body = String::from_utf8(ledger.stdout).unwrap();
    assert!(
        ledger.status.success(),
        "{}",
        String::from_utf8_lossy(&ledger.stderr)
    );
    assert!(
        body.contains("ledger_error") && body.contains("line 2"),
        "{body}"
    );
    let brief = cli(&dir, &["brief", "--agent", "main"]);
    let text = String::from_utf8(brief.stdout).unwrap();
    assert!(brief.status.success(), "{text}");
    assert!(
        text.contains("line 2") && text.contains("# Libellus brief"),
        "{text}"
    );
    assert!(
        text.contains("(1 ledger entry)") && body.contains("\"entries\":1"),
        "brief and ledger must report the same count\nbrief: {text}\nledger: {body}"
    );
    let check = cli(&dir, &["check"]);
    assert_eq!(check.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&check.stderr).contains("line 2"));
}

#[test]
fn one_resolve_wins_when_many_run_together() {
    let dir = project("reg-race");
    init(&dir);
    let made = cli(
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
            "Race?",
        ],
    );
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                cli(
                    &dir,
                    &[
                        "resolve",
                        "--agent",
                        "main",
                        "--id",
                        "q1",
                        "--outcome",
                        &format!("winner {i}"),
                    ],
                )
            })
        })
        .collect();
    let mut wins = 0;
    for handle in handles {
        let out = handle.join().unwrap();
        if out.status.success() {
            wins += 1;
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            assert!(
                err.contains("already resolved") || err.contains("locked"),
                "{err}"
            );
        }
    }
    assert_eq!(wins, 1);
    let text = fs::read_to_string(dir.join(".libellus/agents/main/notes/q1.md")).unwrap();
    assert_eq!(text.matches("Outcome:").count(), 1, "{text}");
}

#[test]
fn a_stale_resolve_lock_is_replaced_and_a_fresh_one_is_reported() {
    let dir = project("reg-lock");
    init(&dir);
    let made = cli(
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
            "Lock?",
        ],
    );
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let lock = dir.join(".libellus/agents/main/notes/.q1.md.lock");
    let file = fs::File::create(&lock).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(120))
        .unwrap();
    drop(file);
    let out = cli(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "q1",
            "--outcome",
            "Took the stale lock.",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!lock.exists());

    let made = cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "question",
            "--id",
            "q2",
            "--summary",
            "Busy?",
        ],
    );
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    fs::write(
        dir.join(".libellus/agents/main/notes/.q2.md.lock"),
        "held\n",
    )
    .unwrap();
    let out = cli(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "q2",
            "--outcome",
            "Should wait.",
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("locked"));
}

fn agent_entries(body: &str) -> u64 {
    let key = "\"agent_total\":{\"entries\":";
    let rest = body.split(key).nth(1).unwrap_or("");
    rest.split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap_or("")
        .parse()
        .unwrap_or(0)
}

#[test]
fn concurrent_spends_all_land_and_each_total_includes_itself() {
    let dir = project("reg-spend");
    init(&dir);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                let mut seen = Vec::new();
                for _ in 0..10 {
                    let out = cli(
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
                    let err = String::from_utf8_lossy(&out.stderr).to_string();
                    assert!(!err.contains("partial line"), "{err}");
                    assert!(out.status.success(), "{err}");
                    let body = String::from_utf8(out.stdout).unwrap();
                    seen.push(agent_entries(&body));
                }
                seen
            })
        })
        .collect();
    let mut counts = Vec::new();
    for handle in handles {
        counts.extend(handle.join().unwrap());
    }
    counts.sort_unstable();
    let unique = {
        let mut u = counts.clone();
        u.dedup();
        u
    };
    let text = fs::read_to_string(dir.join(".libellus/agents/main/ledger.jsonl")).unwrap();
    assert_eq!(
        unique.len(),
        counts.len(),
        "agent_total.entries collided: {counts:?} file_lines={}",
        text.lines().count()
    );
    assert_eq!(counts, (1..=counts.len() as u64).collect::<Vec<_>>());
    let text = fs::read_to_string(dir.join(".libellus/agents/main/ledger.jsonl")).unwrap();
    assert_eq!(text.lines().count(), counts.len(), "{text}");
    assert!(text.ends_with('\n'));
}

#[test]
fn a_huge_note_is_refused_and_a_foreign_agent_field_is_skipped() {
    let dir = project("reg-cap");
    init(&dir);
    let made = cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--id",
            "big",
            "--summary",
            "Big",
        ],
    );
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );
    let path = dir.join(".libellus/agents/main/notes/big.md");
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(&vec![b'x'; 1_048_577]).unwrap();
    drop(file);
    let got = cli(&dir, &["get", "--agent", "main", "--id", "big"]);
    let err = String::from_utf8_lossy(&got.stderr);
    assert_eq!(got.status.code(), Some(1));
    assert!(err.contains("byte cap"), "{err}");

    fs::write(
        &path,
        "---\nkind: finding\nstatus: resolved\nsummary: Big\nagent: other\ncreated: 2026-11-25T00:00:00Z\n---\n\nbody\n",
    )
    .unwrap();
    let listed = cli(&dir, &["ls", "--agent", "main"]);
    let body = String::from_utf8(listed.stdout).unwrap();
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    assert!(
        body.contains("\"total\":0") && body.contains("skipped note"),
        "{body}"
    );
    let brief = cli(&dir, &["brief", "--agent", "main"]);
    let text = String::from_utf8(brief.stdout).unwrap();
    assert!(
        text.contains("skipped note") && text.contains("agent field"),
        "{text}"
    );
    let check = cli(&dir, &["check"]);
    assert_eq!(check.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&check.stderr).contains("agent field"));
    let got = cli(&dir, &["get", "--agent", "main", "--id", "big"]);
    assert!(String::from_utf8_lossy(&got.stderr).contains("belongs to agent"));
}

#[test]
fn closed_stdout_does_not_panic_and_limit_zero_is_rejected() {
    let dir = project("reg-pipe");
    init(&dir);
    let mut child = Command::new(CLI)
        .arg("--version")
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));

    let bad = cli(&dir, &["ls", "--agent", "main", "--limit", "0"]);
    let err = String::from_utf8_lossy(&bad.stderr);
    assert_eq!(bad.status.code(), Some(1));
    assert!(
        err.contains("limit is 0") && err.contains("from 1 up to 18446744073709551615"),
        "{err}"
    );
    let missing = cli(&dir, &["brief"]);
    assert!(String::from_utf8_lossy(&missing.stderr).contains("LIBELLUS_AGENT"));
}

#[test]
fn list_pages_with_a_stable_cursor_and_check_is_a_tool() {
    let dir = project("reg-page");
    init(&dir);
    for id in ["n1", "n2", "n3", "n4", "n5"] {
        let out = cli(
            &dir,
            &[
                "note",
                "--agent",
                "main",
                "--kind",
                "finding",
                "--id",
                id,
                "--summary",
                id,
            ],
        );
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let first = cli(&dir, &["ls", "--agent", "main", "--limit", "2"]);
    let body = String::from_utf8(first.stdout).unwrap();
    assert!(body.contains("\"truncated\":true"), "{body}");
    assert!(body.contains("\"next_offset\":2"), "{body}");
    // Since 1.2.2 the cursor carries the note's place: id@created@seq.
    assert!(body.contains("\"next_before_id\":\"n4@"), "{body}");
    let second = cli(
        &dir,
        &["ls", "--agent", "main", "--limit", "2", "--before_id", "n4"],
    );
    let body = String::from_utf8(second.stdout).unwrap();
    assert!(
        body.contains("n2") && body.contains("n3") && !body.contains("n4"),
        "{body}"
    );
    let listed = talk(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
    );
    // Since 1.3.1 the store check is part of libellus_doctor.
    assert!(listed[0].contains("libellus_doctor"), "{}", listed[0]);
    assert!(!listed[0].contains("libellus_check"), "{}", listed[0]);
    assert!(listed[0].contains("\"minimum\":1"), "{}", listed[0]);
    assert!(
        !listed[0].contains("\"host\""),
        "host is CLI only: {}",
        listed[0]
    );
    let checked = talk(&dir, call("libellus_doctor", "{}", 2).as_bytes());
    assert!(
        is_ok(&checked[0]) && checked[0].contains("store_files"),
        "{}",
        checked[0]
    );
}
