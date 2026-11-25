//! Regression tests for the fifth review (1.2.2): overflow line numbers,
//! case in the agent-tools line, the ls cursor and next_offset, extra
//! arguments, hook flags, an empty LIBELLUS_DIR, groups before init, CRLF
//! notes, pid liveness without ps, the cache default, promote through a
//! symlinked .swarmnotes, Windows device names, and client responses.
mod common;
use common::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cmd(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(CLI);
    c.args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .env_remove("LIBELLUS_AGENT")
        .env_remove("LIBELLUS_TOOLS")
        .env_remove("LIBELLUS_DISABLE")
        .env_remove("SWARMNOTES_DIR");
    c
}

fn cli(dir: &Path, args: &[&str]) -> Output {
    cmd(dir, args).output().unwrap()
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

fn note(dir: &Path, id: &str, kind: &str) {
    ok(&cli(
        dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            kind,
            "--id",
            id,
            "--summary",
            id,
        ],
    ));
}

/// The string value of `key` in a flat JSON reply.
fn string_field(body: &str, key: &str) -> String {
    let pat = format!("\"{key}\":\"");
    let at = body.find(&pat).unwrap_or_else(|| panic!("{key} in {body}")) + pat.len();
    body[at..].split('"').next().unwrap().to_string()
}

/// Ids in an ls reply, in order.
fn listed_ids(body: &str) -> Vec<String> {
    body.split("{\"id\":\"")
        .skip(1)
        .map(|s| s.split('"').next().unwrap().to_string())
        .collect()
}

fn ledger_line(agent: &str, tokens_in: u64) -> String {
    format!(
        "{{\"ts\":\"2026-11-25T02:07:00Z\",\"agent\":\"{agent}\",\"task\":\"t\",\"tokens_in\":{tokens_in},\"tokens_out\":0,\"cost_micro_usd\":0}}\n"
    )
}

#[test]
fn b1_overflow_names_the_file_line_and_report_keeps_both_problems() {
    let dir = project("r5-overflow");
    init(&dir);
    let agent_dir = dir.join(".libellus/agents/main");
    fs::create_dir_all(&agent_dir).unwrap();
    let text = [
        ledger_line("other", 1),
        ledger_line("other", 1),
        ledger_line("main", u64::MAX),
        ledger_line("main", 1),
    ]
    .concat();
    fs::write(agent_dir.join("ledger.jsonl"), text).unwrap();

    let spent = err_text(&cli(
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
    assert!(spent.contains("line 4"), "{spent}");
    assert!(!spent.contains("line 2"), "{spent}");

    let report = ok(&cli(&dir, &["ledger", "--agent", "main"]));
    assert!(report.contains("line 4: totals overflowed"), "{report}");
    assert!(
        report.contains("line 1: agent \\\"other\\\""),
        "the left-out line warning must stay beside the overflow: {report}"
    );

    let brief = ok(&cli(&dir, &["brief", "--agent", "main"]));
    assert!(brief.contains("line 4"), "{brief}");

    let check = err_text(&cli(&dir, &["check"]));
    assert!(check.contains("line 4: totals overflowed"), "{check}");
}

#[test]
fn b2_the_agent_tools_line_ignores_case() {
    let dir = project("r5-case");
    init(&dir);
    fs::write(
        dir.join("AGENTS.md"),
        "# x\n\n```agent-tools\nLibellus: Disable Ledger\n```\n",
    )
    .unwrap();
    let listed = talk(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
    );
    assert!(listed[0].contains("libellus_note"), "{}", listed[0]);
    assert!(!listed[0].contains("libellus_spend"), "{}", listed[0]);
    assert!(!listed[0].contains("NOTICE"), "{}", listed[0]);
}

#[test]
fn b3_before_id_survives_a_resolve_and_a_delete() {
    let dir = project("r5-cursor");
    init(&dir);
    for id in ["q1", "q2", "q3"] {
        note(&dir, id, "question");
    }
    let first = ok(&cli(
        &dir,
        &["ls", "--agent", "main", "--status", "open", "--limit", "1"],
    ));
    assert_eq!(listed_ids(&first), ["q3"]);
    let cursor = string_field(&first, "next_before_id");
    ok(&cli(
        &dir,
        &[
            "resolve",
            "--agent",
            "main",
            "--id",
            "q3",
            "--outcome",
            "done",
        ],
    ));
    for before in [cursor.as_str(), "q3"] {
        let next = ok(&cli(
            &dir,
            &[
                "ls",
                "--agent",
                "main",
                "--status",
                "open",
                "--limit",
                "1",
                "--before_id",
                before,
            ],
        ));
        assert_eq!(listed_ids(&next), ["q2"], "{before}: {next}");
    }

    for id in ["f1", "f2", "f3", "f4"] {
        note(&dir, id, "finding");
    }
    let page = ok(&cli(
        &dir,
        &["ls", "--agent", "main", "--kind", "finding", "--limit", "2"],
    ));
    assert_eq!(listed_ids(&page), ["f3", "f4"]);
    let cursor = string_field(&page, "next_before_id");
    fs::remove_file(dir.join(".libellus/agents/main/notes/f3.md")).unwrap();
    let next = ok(&cli(
        &dir,
        &[
            "ls",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--before_id",
            &cursor,
        ],
    ));
    assert_eq!(listed_ids(&next), ["f1", "f2"], "{next}");
}

#[test]
fn b4_next_offset_is_absolute_on_a_cursor_page() {
    let dir = project("r5-offset");
    init(&dir);
    for i in 1..=6 {
        note(&dir, &format!("n-{i}"), "finding");
    }
    let page = ok(&cli(
        &dir,
        &[
            "ls",
            "--agent",
            "main",
            "--limit",
            "2",
            "--before_id",
            "n-5",
        ],
    ));
    assert_eq!(listed_ids(&page), ["n-3", "n-4"]);
    assert!(page.contains("\"next_offset\":4"), "{page}");
    let by_offset = ok(&cli(
        &dir,
        &["ls", "--agent", "main", "--limit", "2", "--offset", "4"],
    ));
    assert_eq!(listed_ids(&by_offset), ["n-1", "n-2"]);
    let plain = ok(&cli(&dir, &["ls", "--agent", "main", "--limit", "2"]));
    assert!(plain.contains("\"next_offset\":2"), "{plain}");
}

#[test]
fn b5_commands_without_flags_refuse_extra_arguments() {
    let dir = project("r5-extra");
    init(&dir);
    for args in [
        &["check", "--bogus"][..],
        &["init", "--bogus"],
        &["--version", "x"],
        &["mcp", "x"],
        &["serve-mcp", "--x"],
        &["help", "x"],
    ] {
        let out = cli(&dir, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", err_text(&out));
        assert!(err_text(&out).contains("takes no arguments"), "{args:?}");
    }
    ok(&cli(&dir, &["check"]));
}

#[test]
fn b6_hook_uses_the_shared_flag_rules() {
    let dir = project("r5-hook");
    let text = ok(&cli(&dir, &["hook", "--agent=main", "--host=claude"]));
    assert!(text.contains("libellus brief --agent main"), "{text}");
    let twice = cli(
        &dir,
        &[
            "hook", "--agent", "main", "--host", "claude", "--host", "cursor",
        ],
    );
    assert_eq!(twice.status.code(), Some(2));
    assert!(err_text(&twice).contains("given twice"));
    let unknown = err_text(&cli(&dir, &["hook", "--agent", "main", "--colour", "red"]));
    assert!(unknown.contains("unknown flag --colour"), "{unknown}");
}

#[test]
fn b7_an_empty_libellus_dir_is_unset() {
    let dir = project("r5-emptydir");
    init(&dir);
    note(&dir, "seen", "finding");
    let out = cmd(&dir, &["ls", "--agent", "main"])
        .env("LIBELLUS_DIR", "")
        .output()
        .unwrap();
    assert!(ok(&out).contains("seen"));
    let fresh = project("r5-emptydir-init");
    let made = cmd(&fresh, &["init"])
        .env("LIBELLUS_DIR", "")
        .output()
        .unwrap();
    ok(&made);
    assert!(fresh.join(".libellus/agents").is_dir());
}

#[test]
fn b8_groups_apply_before_a_store_exists() {
    let dir = project("r5-nostore");
    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nlibellus: disable ledger\n```\n",
    )
    .unwrap();
    let sub = dir.join("src/deep");
    fs::create_dir_all(&sub).unwrap();
    for at in [&dir, &sub] {
        let listed = talk(
            at,
            b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
        );
        assert!(listed[0].contains("libellus_note"), "{}", listed[0]);
        assert!(!listed[0].contains("libellus_spend"), "{}", listed[0]);
    }
}

#[test]
fn b9_a_crlf_note_is_read() {
    let dir = project("r5-crlf");
    init(&dir);
    note(&dir, "plain", "finding");
    let notes = dir.join(".libellus/agents/main/notes");
    let text = "---\r\nkind: decision\r\nstatus: resolved\r\nsummary: Saved on Windows\r\nagent: main\r\ncreated: 2026-11-25T02:05:00Z\r\n---\r\n\r\nLine one.\r\nLine two.\r\n";
    fs::write(notes.join("windows.md"), text).unwrap();
    let listed = ok(&cli(&dir, &["ls", "--agent", "main", "--bodies"]));
    assert!(
        listed.contains("windows") && listed.contains("plain"),
        "{listed}"
    );
    assert!(listed.contains("Line one.\\nLine two."), "{listed}");
    assert!(!listed.contains("\\r"), "{listed}");
    let got = ok(&cli(&dir, &["get", "--agent", "main", "--id", "windows"]));
    assert!(got.contains("Saved on Windows"), "{got}");
    let brief = ok(&cli(&dir, &["brief", "--agent", "main"]));
    assert!(brief.contains("Saved on Windows"), "{brief}");
    ok(&cli(&dir, &["check"]));
    assert_eq!(
        fs::read_to_string(notes.join("windows.md")).unwrap(),
        text,
        "a read must not rewrite the file"
    );
}

#[cfg(unix)]
#[test]
fn b10_a_live_lock_holder_is_not_judged_by_ps() {
    use std::os::unix::fs::PermissionsExt;
    let dir = project("r5-ps");
    init(&dir);
    note(&dir, "seed", "finding");
    // A ps that fails for every call, like one without -p.
    let bin = project("r5-ps-bin");
    let fake = bin.join("ps");
    fs::write(&fake, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let lock = dir.join(".libellus/agents/main/.seq.lock");
    let line = format!("pid {} live-holder\n", std::process::id());
    fs::write(&lock, &line).unwrap();
    let out = cmd(&dir, &["check"]).env("PATH", &path).output().unwrap();
    ok(&out);
    assert_eq!(
        fs::read_to_string(&lock).unwrap(),
        line,
        "a lock held by a live process was broken"
    );
    // A dead holder is still seen as dead.
    fs::write(&lock, "pid 999999\n").unwrap();
    ok(&cmd(&dir, &["check"]).env("PATH", &path).output().unwrap());
    assert!(!lock.exists());
}

#[test]
fn a_cache_without_tasks_complete_is_not_trusted_for_task_totals() {
    let dir = project("r5-meta");
    init(&dir);
    for _ in 0..2 {
        ok(&cli(
            &dir,
            &[
                "spend",
                "--agent",
                "main",
                "--task",
                "a",
                "--tokens_in",
                "1",
            ],
        ));
    }
    let meta = dir.join(".libellus/agents/main/ledger.meta");
    let text = fs::read_to_string(&meta).unwrap();
    assert!(text.contains("\"tasks_complete\":true"), "{text}");
    let cut = text.find("\"tasks\":{").unwrap();
    let old = format!("{}\"tasks\":{{}}}}", &text[..cut]).replace("\"tasks_complete\":true,", "");
    fs::write(&meta, old).unwrap();
    let out = ok(&cli(
        &dir,
        &[
            "spend",
            "--agent",
            "main",
            "--task",
            "a",
            "--tokens_in",
            "1",
        ],
    ));
    assert!(
        out.contains("\"task_total\":{\"entries\":3,\"tokens_in\":3"),
        "{out}"
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_swarmnotes_found_by_walking_is_refused() {
    let dir = project("r5-swarm-link");
    init(&dir);
    let real = project("r5-swarm-real");
    fs::create_dir_all(real.join(".swarmnotes")).unwrap();
    std::os::unix::fs::symlink(real.join(".swarmnotes"), dir.join(".swarmnotes")).unwrap();
    note(&dir, "pick", "decision");
    let out = err_text(&cli(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "pick", "--scope", "net",
        ],
    ));
    assert!(out.contains("symlink"), "{out}");
    assert!(!real.join(".swarmnotes/net").exists());
    // SWARMNOTES_DIR is an explicit choice and may lead through a symlink.
    let via = cmd(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "pick", "--scope", "net",
        ],
    )
    .env("SWARMNOTES_DIR", dir.join(".swarmnotes"))
    .output()
    .unwrap();
    ok(&via);
    assert!(real.join(".swarmnotes/net/pick.md").is_file());
}

#[test]
fn windows_device_names_are_refused_everywhere() {
    let dir = project("r5-devices");
    init(&dir);
    for agent in ["con", "nul", "com1", "lpt9"] {
        let out = err_text(&cli(
            &dir,
            &[
                "note",
                "--agent",
                agent,
                "--kind",
                "finding",
                "--summary",
                "x",
            ],
        ));
        assert!(out.contains("device name"), "{agent}: {out}");
    }
    for id in ["aux", "prn/x", "x/com3"] {
        let out = err_text(&cli(
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
                "x",
            ],
        ));
        assert!(out.contains("device name"), "{id}: {out}");
    }
    let made = ok(&cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "finding",
            "--summary",
            "Con",
        ],
    ));
    assert!(made.contains("\"id\":\"con-note\""), "{made}");
    // Names that only start like a device are fine.
    ok(&cli(
        &dir,
        &[
            "note",
            "--agent",
            "console",
            "--kind",
            "finding",
            "--id",
            "com10",
            "--summary",
            "x",
        ],
    ));
    fs::create_dir_all(dir.join(".swarmnotes")).unwrap();
    let scope = err_text(&cli(
        &dir,
        &[
            "promote", "--agent", "main", "--id", "con-note", "--scope", "aux",
        ],
    ));
    assert!(scope.contains("device name"), "{scope}");
    ok(&cli(&dir, &["check"]));
}

#[test]
fn a_client_response_gets_no_reply() {
    let dir = project("r5-response");
    init(&dir);
    let replies = talk(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":5,\"result\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":6,\"error\":{\"code\":-1,\"message\":\"no\"}}\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n{\"jsonrpc\":\"2.0\",\"id\":8}\n",
    );
    assert_eq!(replies.len(), 2, "{replies:?}");
    assert!(replies[0].contains("\"id\":7"), "{replies:?}");
    assert!(
        replies[1].contains("\"id\":8") && replies[1].contains("-32600"),
        "{replies:?}"
    );
}

#[test]
fn the_crate_doc_names_files_that_exist() {
    let lib = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    assert!(!lib.contains("basis.md"));
    for name in ["SPEC.md", "README.md"] {
        assert!(lib.contains(name));
        assert!(Path::new(env!("CARGO_MANIFEST_DIR")).join(name).is_file());
    }
}
