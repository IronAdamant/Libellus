//! Operating-system locks across processes: a waiter gets the lock when the
//! holder lets go, a killed holder frees it at once, many writers lose
//! nothing, and lock files from 1.3 block nothing.
//!
//! The holder is this test binary run again with `LIBELLUS_TEST_HOLD` set: it
//! takes the named store lock, prints `held`, and keeps it until a `release`
//! file appears in the project folder. (Not stdin: a pipe end can leak into
//! a child that another test thread starts at the same moment, and then EOF
//! never comes.)

mod common;
use common::*;
use libellus::store::{Locked, Store};
use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Not a test of its own: the body runs only in a child started by `hold`.
#[test]
fn holder() {
    let Ok(what) = std::env::var("LIBELLUS_TEST_HOLD") else {
        return;
    };
    let store = Store::open().unwrap();
    let lock = match what.as_str() {
        "notes" => Locked::Notes("main"),
        "ledger" => Locked::Ledger("main"),
        "seq" => Locked::Seq("main"),
        _ => Locked::Board,
    };
    let _held = store.lock(lock).unwrap();
    println!("held");
    let release = store.project.join("release");
    let start = Instant::now();
    while !release.exists() && start.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Start a process that holds `what` in the store under `dir`, and return
/// once it holds it.
fn hold(dir: &Path, what: &str) -> Child {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "holder", "--nocapture", "--test-threads=1"])
        .env("LIBELLUS_TEST_HOLD", what)
        .env("LIBELLUS_DIR", dir.join(".libellus"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    loop {
        match lines.next() {
            // The harness may print "test holder ... " first on the same line.
            Some(Ok(line)) if line.trim_end().ends_with("held") => break,
            Some(Ok(_)) => continue,
            _ => panic!("the holder exited before it held {what}"),
        }
    }
    // Keep reading so the holder never blocks on a full pipe.
    std::thread::spawn(move || for _ in lines {});
    child
}

fn cli(dir: &Path, args: &[&str]) -> Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .env_remove("LIBELLUS_AGENT")
        .output()
        .unwrap()
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn note(dir: &Path, id: &str) {
    ok(&cli(
        dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "question",
            "--id",
            id,
            "--summary",
            "Locked?",
        ],
    ));
}

#[test]
fn a_waiter_gets_the_lock_when_the_holder_lets_go() {
    let dir = project("os-contend");
    init(&dir);
    note(&dir, "q1");
    let mut holder = hold(&dir, "notes");
    // Busy for anyone else, in this process too.
    let path = dir.join(".libellus/locks/main.notes.lock");
    let err = libellus::oslock::FileLock::acquire(&path, Duration::from_millis(50), "libellus")
        .unwrap_err();
    assert!(err.contains("locked by another libellus process"), "{err}");

    let started = Instant::now();
    let mut resolve = Command::new(CLI)
        .args([
            "resolve",
            "--agent",
            "main",
            "--id",
            "q1",
            "--outcome",
            "Waited.",
        ])
        .current_dir(&dir)
        .env_remove("LIBELLUS_DIR")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        resolve.try_wait().unwrap().is_none(),
        "resolve did not wait for the held lock"
    );
    fs::write(dir.join("release"), b"").unwrap();
    holder.wait().unwrap();
    ok(&resolve.wait_with_output().unwrap());
    assert!(started.elapsed() >= Duration::from_millis(400));
    assert!(started.elapsed() < Duration::from_secs(6));
    let text = fs::read_to_string(dir.join(".libellus/agents/main/notes/q1.md")).unwrap();
    assert!(text.contains("Waited."), "{text}");
    assert!(path.is_file(), "a lock file is never deleted");
}

#[test]
fn a_killed_holder_frees_the_lock_at_once() {
    let dir = project("os-kill");
    init(&dir);
    for what in ["ledger", "seq", "board"] {
        let mut holder = hold(&dir, what);
        // SIGKILL on Unix, TerminateProcess on Windows: no cleanup runs.
        holder.kill().unwrap();
        holder.wait().unwrap();
        let started = Instant::now();
        match what {
            "ledger" => {
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
            }
            "seq" => note(&dir, "after-kill"),
            _ => {
                ok(&cli(
                    &dir,
                    &["plan", "--agent", "main", "--outline", "- [ ] After kill\n"],
                ));
            }
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{what} waited {:?} after its holder was killed",
            started.elapsed()
        );
    }
}

#[test]
fn many_writers_lose_nothing_and_seq_stays_unique() {
    let dir = project("os-stress");
    init(&dir);
    const WORKERS: usize = 8;
    const ROUNDS: usize = 6;
    let workers: Vec<_> = (0..WORKERS)
        .map(|w| {
            let dir = dir.clone();
            std::thread::spawn(move || {
                for r in 0..ROUNDS {
                    let id = format!("w{w}-r{r}");
                    note(&dir, &id);
                    ok(&cli(
                        &dir,
                        &[
                            "spend",
                            "--agent",
                            "main",
                            "--task",
                            "stress",
                            "--tokens_in",
                            "1",
                        ],
                    ));
                    ok(&cli(
                        &dir,
                        &[
                            "plan",
                            "--agent",
                            "main",
                            "--outline",
                            &format!("- [ ] Task w{w} r{r}\n"),
                        ],
                    ));
                    ok(&cli(
                        &dir,
                        &[
                            "resolve",
                            "--agent",
                            "main",
                            "--id",
                            &id,
                            "--outcome",
                            "Done.",
                        ],
                    ));
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    let total = WORKERS * ROUNDS;
    let seqs = |folder: &Path| -> Vec<u64> {
        fs::read_dir(folder)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
            .map(|e| {
                let text = fs::read_to_string(e.path()).unwrap();
                let line = text.lines().find(|l| l.starts_with("seq: ")).unwrap();
                line[5..].trim().parse().unwrap()
            })
            .collect()
    };
    let notes_dir = dir.join(".libellus/agents/main/notes");
    let note_seqs = seqs(&notes_dir);
    assert_eq!(note_seqs.len(), total);
    assert_eq!(
        note_seqs.iter().collect::<BTreeSet<_>>().len(),
        total,
        "two notes share a seq"
    );
    for e in fs::read_dir(&notes_dir).unwrap().flatten() {
        let text = fs::read_to_string(e.path()).unwrap();
        assert_eq!(text.matches("Done.").count(), 1, "{text}");
    }
    let board_seqs = seqs(&dir.join(".libellus/board"));
    assert_eq!(board_seqs.len(), total);
    assert_eq!(
        board_seqs.iter().collect::<BTreeSet<_>>().len(),
        total,
        "two board tasks share a seq"
    );
    let ledger = fs::read_to_string(dir.join(".libellus/agents/main/ledger.jsonl")).unwrap();
    assert_eq!(ledger.lines().count(), total);
    let report = ok(&cli(&dir, &["ledger", "--agent", "main"]));
    assert!(
        report.contains(&format!("\"tokens_in\":{total}")),
        "{report}"
    );
    ok(&cli(&dir, &["check"]));
}

#[test]
fn lock_files_from_1_3_block_nothing_and_check_removes_them() {
    let dir = project("os-old");
    init(&dir);
    note(&dir, "q1");
    // 1.3 lock files naming a live process (this one): 1.3 would have waited
    // up to 30 seconds on each.
    let line = format!("pid {} old-token\n", std::process::id());
    let old = [
        ".libellus/agents/main/.seq.lock",
        ".libellus/agents/main/.ledger.jsonl.lock",
        ".libellus/agents/main/notes/.q1.md.lock",
        ".libellus/agents/main/notes/.q1.md.lock.break",
        ".libellus/board/.board.lock",
    ];
    fs::create_dir_all(dir.join(".libellus/board")).unwrap();
    for rel in old {
        fs::write(dir.join(rel), &line).unwrap();
    }
    let started = Instant::now();
    note(&dir, "q2");
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
    ok(&cli(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "q1",
            "--outcome",
            "Not blocked.",
        ],
    ));
    ok(&cli(
        &dir,
        &[
            "plan",
            "--agent",
            "main",
            "--outline",
            "- [ ] Not blocked\n",
        ],
    ));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    // doctor reports them and changes nothing; check removes them.
    let out = cli(&dir, &["doctor"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Libellus 1.3"));
    assert!(old.iter().all(|rel| dir.join(rel).exists()));
    let body = ok(&cli(&dir, &["check"]));
    for rel in old {
        assert!(!dir.join(rel).exists(), "{rel}: {body}");
    }
    // The 1.4 lock files are normal: they stay and doctor passes.
    for name in [
        "main.seq.lock",
        "main.ledger.lock",
        "main.notes.lock",
        "board.lock",
    ] {
        assert!(dir.join(".libellus/locks").join(name).is_file(), "{name}");
    }
    let out = cli(&dir, &["doctor"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("\"name\":\"stray_files\",\"ok\":true"),
        "{text}"
    );
}

#[test]
fn temp_files_are_reaped_by_age_only() {
    let dir = project("os-temp");
    init(&dir);
    note(&dir, "q1");
    let agent = dir.join(".libellus/agents/main");
    let set_age = |p: &Path, secs: u64| {
        fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(secs))
            .unwrap();
    };
    // Over an hour old: gone, whatever pid it names.
    let old = agent.join(format!(".ledger.jsonl.tmp-{}-1", std::process::id()));
    fs::write(&old, b"leftover").unwrap();
    set_age(&old, 2 * 60 * 60);
    // Fresh, naming a pid that is not running: kept, a writer may own it.
    let fresh = agent.join(".ledger.jsonl.tmp-999999-1");
    fs::write(&fresh, b"in progress").unwrap();
    set_age(&fresh, 120);
    let body = ok(&cli(&dir, &["check"]));
    assert!(!old.exists(), "{body}");
    assert!(fresh.exists(), "{body}");
    assert!(body.contains(".ledger.jsonl.tmp-999999-1"), "{body}");
}
