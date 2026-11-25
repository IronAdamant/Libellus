//! Regression tests for the fourth review (1.2.1): lock takeover, promote
//! under the note lock, mixed seq ordering, symlinked folders, a FIFO cache,
//! typed ledger fields, repeated flags, and bracketed promote summaries.
mod common;
use common::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .env_remove("LIBELLUS_AGENT")
        .env_remove("SWARMNOTES_DIR")
        .output()
        .unwrap()
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn err_text(out: &Output) -> String {
    assert!(!out.status.success(), "expected failure");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn writers_racing_past_a_dead_lock_still_get_distinct_seq() {
    let dir = project("r4-seq");
    init(&dir);
    ok(&cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--summary",
            "seed",
        ],
    ));
    let lock = dir.join(".libellus/agents/main/.seq.lock");
    for round in 0..6 {
        fs::write(&lock, "pid 999999\n").unwrap();
        let children: Vec<_> = (0..6)
            .map(|i| {
                Command::new(CLI)
                    .args([
                        "note",
                        "--agent",
                        "main",
                        "--kind",
                        "finding",
                        "--summary",
                        &format!("r{round} n{i}"),
                    ])
                    .current_dir(&dir)
                    .env_remove("LIBELLUS_DIR")
                    .output()
            })
            .collect();
        for child in children {
            ok(&child.unwrap());
        }
    }
    let mut seqs = Vec::new();
    for entry in fs::read_dir(dir.join(".libellus/agents/main/notes")).unwrap() {
        let text = fs::read_to_string(entry.unwrap().path()).unwrap();
        let line = text.lines().find(|l| l.starts_with("seq: ")).unwrap();
        seqs.push(line.to_string());
    }
    let total = seqs.len();
    seqs.sort();
    seqs.dedup();
    assert_eq!(total, 37);
    assert_eq!(seqs.len(), total, "two notes share a seq");
}

#[test]
fn concurrent_resolve_and_promote_lose_nothing() {
    for trial in 0..5 {
        let dir = project(&format!("r4-promote-{trial}"));
        init(&dir);
        fs::create_dir(dir.join(".swarmnotes")).unwrap();
        ok(&cli(
            &dir,
            &[
                "note",
                "--agent",
                "main",
                "--kind",
                "decision",
                "--id",
                "d1",
                "--summary",
                "use retries",
            ],
        ));
        let spawn = |args: &[&str]| {
            Command::new(CLI)
                .args(args)
                .current_dir(&dir)
                .env_remove("LIBELLUS_DIR")
                .env_remove("SWARMNOTES_DIR")
                .spawn()
                .unwrap()
        };
        let mut kids = vec![
            spawn(&[
                "resolve",
                "--agent",
                "main",
                "--id",
                "d1",
                "--supersede",
                "--outcome",
                "SUPERSEDED: by d2",
            ]),
            spawn(&["promote", "--agent", "main", "--id", "d1", "--scope", "s"]),
            spawn(&["promote", "--agent", "main", "--id", "d1", "--scope", "s"]),
        ];
        for kid in &mut kids {
            kid.wait().unwrap();
        }
        let note = fs::read_to_string(dir.join(".libellus/agents/main/notes/d1.md")).unwrap();
        assert!(note.contains("superseded: true"), "resolve lost:\n{note}");
        assert!(note.contains("promoted: s/d1\n"), "promote lost:\n{note}");
        let written: Vec<_> = fs::read_dir(dir.join(".swarmnotes/s")).unwrap().collect();
        assert_eq!(written.len(), 1, "a second Relatum note was written");
    }
}

#[test]
fn notes_with_and_without_seq_sort_without_panicking() {
    let dir = project("r4-sort");
    init(&dir);
    ok(&cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--summary",
            "seed",
        ],
    ));
    let notes = dir.join(".libellus/agents/main/notes");
    // seq order runs against created order, which a clock step produces.
    let mut x: u64 = 7;
    for i in 0..400u64 {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let minute = (x >> 33) % 50;
        let second = (x >> 20) % 60;
        let seq = if i % 2 == 1 {
            format!("seq: {}\n", 100 + i)
        } else {
            String::new()
        };
        fs::write(
            notes.join(format!("n{i}.md")),
            format!(
                "---\nkind: finding\nstatus: resolved\nsummary: n{i}\nagent: main\ncreated: 2026-11-25T10:{minute:02}:{second:02}Z\n{seq}---\n"
            ),
        )
        .unwrap();
    }
    let out = ok(&cli(&dir, &["ls", "--agent", "main", "--limit", "500"]));
    assert!(out.contains("\"total\":401"), "{out}");
    ok(&cli(&dir, &["brief", "--agent", "main"]));
    ok(&cli(&dir, &["check"]));
    // Notes with seq stay in seq order relative to each other.
    let order: Vec<u64> = out
        .split("\"seq\":")
        .skip(1)
        .map(|s| {
            s.split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);
}

#[cfg(unix)]
#[test]
fn a_symlinked_notes_or_archive_folder_is_refused() {
    let dir = project("r4-link");
    init(&dir);
    ok(&cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "t",
            "--tokens_in",
            "1",
        ],
    ));
    let outside = dir.join("outside");
    fs::create_dir(&outside).unwrap();
    let agent = dir.join(".libellus/agents/main");
    let _ = fs::remove_dir_all(agent.join("notes"));
    std::os::unix::fs::symlink(&outside, agent.join("notes")).unwrap();
    let out = cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--summary",
            "escape",
        ],
    );
    assert!(err_text(&out).contains("symlink"));
    std::os::unix::fs::symlink(&outside, agent.join("archive")).unwrap();
    let out = cli(&dir, &["archive", "--agent", "main"]);
    assert!(err_text(&out).contains("symlink"));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    assert!(agent.join("ledger.jsonl").exists());
}

#[cfg(unix)]
#[test]
fn a_symlinked_store_is_refused_rather_than_skipped() {
    let parent = project("r4-store");
    init(&parent);
    let child = parent.join("child");
    fs::create_dir(&child).unwrap();
    let elsewhere = parent.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, child.join(".libellus")).unwrap();
    let out = cli(
        &child,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--summary",
            "x",
        ],
    );
    assert!(err_text(&out).contains("not a folder"));
    assert!(!parent.join(".libellus/agents/main/notes").exists());
}

#[cfg(unix)]
#[test]
fn a_fifo_cache_does_not_block_spend() {
    let dir = project("r4-fifo");
    init(&dir);
    ok(&cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "t",
            "--tokens_in",
            "1",
        ],
    ));
    let meta = dir.join(".libellus/agents/main/ledger.meta");
    let _ = fs::remove_file(&meta);
    let made = Command::new("mkfifo").arg(&meta).status().unwrap();
    assert!(made.success());
    let mut child = Command::new(CLI)
        .args([
            "spend",
            "--agent",
            "main",
            "--task",
            "t",
            "--tokens_in",
            "2",
        ])
        .current_dir(&dir)
        .env_remove("LIBELLUS_DIR")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if start.elapsed() > std::time::Duration::from_secs(10) {
            let _ = child.kill();
            panic!("spend blocked on a FIFO ledger.meta");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let out = ok(&cli(&dir, &["ledger", "--agent", "main"]));
    assert!(out.contains("\"tokens_in\":3"), "{out}");
}

#[test]
fn a_ledger_line_with_a_non_string_step_is_damage() {
    let dir = project("r4-typed");
    init(&dir);
    ok(&cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "t",
            "--tokens_in",
            "1",
        ],
    ));
    let ledger = dir.join(".libellus/agents/main/ledger.jsonl");
    let mut text = fs::read_to_string(&ledger).unwrap();
    text.push_str("{\"ts\":\"2026-11-25T00:00:00Z\",\"agent\":\"main\",\"task\":\"t\",\"step\":123,\"tokens_in\":1,\"tokens_out\":0,\"cost_micro_usd\":0}\n");
    fs::write(&ledger, text).unwrap();
    let out = cli(&dir, &["check"]);
    assert!(err_text(&out).contains("\"step\" must be a string"));
}

#[test]
fn a_repeated_number_flag_is_a_usage_error() {
    let dir = project("r4-repeat");
    init(&dir);
    let out = cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "t",
            "--tokens_in",
            "5",
            "--tokens_in",
            "7",
        ],
    );
    assert_eq!(out.status.code(), Some(2), "{}", err_text(&out));
    assert!(err_text(&out).contains("given twice"));
    let out = cli(
        &dir,
        &["ls", "--agent", "main", "--limit", "1", "--limit", "0"],
    );
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn a_bracketed_summary_stays_a_string_in_relatum() {
    let dir = project("r4-bracket");
    init(&dir);
    fs::create_dir(dir.join(".swarmnotes")).unwrap();
    ok(&cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--id",
            "wip",
            "--summary",
            "[WIP] retries apply to [GET]",
        ],
    ));
    ok(&cli(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "wip", "--scope", "net",
        ],
    ));
    let rel = fs::read_to_string(dir.join(".swarmnotes/net/wip.md")).unwrap();
    assert!(
        rel.contains("summary: [WIP] retries apply to [GET].\n"),
        "{rel}"
    );
}
