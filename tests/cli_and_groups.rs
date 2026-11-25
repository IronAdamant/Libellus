mod common;
use common::*;
use std::fs;
use std::process::Command;

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .output()
        .unwrap()
}

#[test]
fn cli_rejects_bad_flags_with_exit_2() {
    let dir = project("cliflags");
    init(&dir);
    assert_eq!(
        cli(
            &dir,
            &["note", "--agent", "main", "--kind", "finding", "--summary"]
        )
        .status
        .code(),
        Some(2)
    );
    assert_eq!(
        cli(&dir, &["ls", "--agent", "main", "--limit", "many"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(cli(&dir, &["frobnicate"]).status.code(), Some(2));
    assert_eq!(cli(&dir, &[]).status.code(), Some(2));
    let unknown = cli(&dir, &["ls", "--agent", "main", "--colour", "red"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("unknown flag"));
    assert!(cli(&dir, &["--version"]).status.success());
}

#[test]
fn cli_brief_prints_text_and_note_round_trips() {
    let dir = project("clibrief");
    init(&dir);
    assert!(cli(
        &dir,
        &[
            "note",
            "--agent",
            "main",
            "--kind",
            "question",
            "--summary",
            "Why is X slow?",
            "--evidence",
            "chisel:a.rs",
            "--evidence",
            "trammel:p/1"
        ]
    )
    .status
    .success());
    let out = cli(&dir, &["brief", "--agent", "main"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("Open questions (1)") && text.contains("Why is X slow?"),
        "{text}"
    );
    let out = cli(&dir, &["check"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn tools_list(dir: &std::path::Path) -> String {
    talk(
        dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
    )
    .remove(0)
}

#[test]
fn agent_tools_block_switches_groups_and_typos_fall_back_to_core_with_a_notice() {
    let dir = project("groups");
    init(&dir);
    assert!(tools_list(&dir).contains("libellus_spend"));

    fs::write(
        dir.join("AGENTS.md"),
        "# x\n\n```agent-tools\nrelatum: disable lint\nlibellus: disable ledger\n```\n",
    )
    .unwrap();
    let l = tools_list(&dir);
    assert!(
        !l.contains("libellus_spend") && l.contains("libellus_note") && !l.contains("NOTICE"),
        "{l}"
    );
    let r = talk(
        &dir,
        call(
            "libellus_spend",
            r#"{"agent":"main","task":"t","tokens_in":1}"#,
            1,
        )
        .as_bytes(),
    );
    assert!(is_error(&r[0]) && r[0].contains("AGENTS.md"), "{}", r[0]);

    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nlibellus: disable ledgr\n```\n",
    )
    .unwrap();
    let l = tools_list(&dir);
    assert!(
        l.contains("NOTICE") && l.contains("unknown group") && !l.contains("libellus_spend"),
        "{l}"
    );

    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nlibellus: disable core\n```\n",
    )
    .unwrap();
    assert!(tools_list(&dir).contains("cannot be disabled"));

    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nlibellus: tools core, ledger\n```\n",
    )
    .unwrap();
    assert!(tools_list(&dir).contains("libellus_spend"));
}

#[test]
fn initialize_reports_the_server() {
    let dir = project("init");
    let r = talk(&dir, b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n");
    assert!(
        r[0].contains("\"name\":\"libellus\"") && r[0].contains("2025-06-18"),
        "{}",
        r[0]
    );
}
