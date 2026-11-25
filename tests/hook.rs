//! The compaction hook snippet, and LIBELLUS_AGENT as a default agent name.
mod common;
use common::*;
use std::process::Command;

fn run(dir: &std::path::Path, args: &[&str], agent: Option<&str>) -> std::process::Output {
    let mut cmd = Command::new(CLI);
    cmd.args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .env_remove("LIBELLUS_AGENT");
    if let Some(agent) = agent {
        cmd.env("LIBELLUS_AGENT", agent);
    }
    cmd.output().unwrap()
}

#[test]
fn hook_prints_the_brief_command() {
    let dir = project("hook-print");
    let out = run(
        &dir,
        &["hook", "--agent", "main", "--host", "generic"],
        None,
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("libellus brief --agent main\n"), "{text}");
    assert!(text.contains("session start") && text.contains("after compaction"));

    let out = run(&dir, &["hook", "--host", "claude"], Some("main"));
    assert!(out.status.success());
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .contains("libellus brief --agent main"));

    let out = run(
        &dir,
        &["hook", "--agent", "other", "--host", "cursor"],
        Some("main"),
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("libellus brief --agent other --host cursor"),
        "{text}"
    );
    assert!(!text.contains("libellus brief --agent main --host cursor"));

    assert_eq!(
        run(&dir, &["hook", "--agent", "main", "--host", "nope"], None)
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run(&dir, &["hook", "--agent", "Main"], None).status.code(),
        Some(2)
    );
    assert_eq!(run(&dir, &["hook"], None).status.code(), Some(2));
    assert_eq!(
        run(&dir, &["hook", "--agent", "main", "--colour", "red"], None)
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn agent_env_fills_required_commands_and_brief_text_stays_text() {
    let dir = project("hook-env");
    init(&dir);
    let made = run(
        &dir,
        &[
            "note",
            "--kind",
            "question",
            "--summary",
            "Does retry apply to POST?",
        ],
        Some("main"),
    );
    assert!(
        made.status.success(),
        "{}",
        String::from_utf8_lossy(&made.stderr)
    );

    let brief = run(&dir, &["brief"], Some("main"));
    let text = String::from_utf8(brief.stdout).unwrap();
    assert!(
        brief.status.success(),
        "{}",
        String::from_utf8_lossy(&brief.stderr)
    );
    assert!(text.starts_with("# Libellus brief for main\n"), "{text}");
    assert!(text.contains("Does retry apply to POST?"));
    assert!(!text.contains("additional_context"));

    let cursor = run(&dir, &["brief", "--host", "cursor"], Some("main"));
    let wrapped = String::from_utf8(cursor.stdout).unwrap();
    assert!(
        cursor.status.success(),
        "{}",
        String::from_utf8_lossy(&cursor.stderr)
    );
    assert!(wrapped.starts_with("{\"additional_context\":"), "{wrapped}");
    assert!(wrapped.contains("Does retry apply to POST?"));
    assert!(wrapped.contains("\\n"), "{wrapped}");

    let missing = run(&dir, &["brief"], None);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("missing required argument"));

    let explicit = run(&dir, &["brief", "--agent", "nobody"], Some("main"));
    let text = String::from_utf8(explicit.stdout).unwrap();
    assert!(
        explicit.status.success(),
        "{}",
        String::from_utf8_lossy(&explicit.stderr)
    );
    assert!(text.starts_with("# Libellus brief for nobody\n"), "{text}");

    let spend_other = run(
        &dir,
        &[
            "spend",
            "--agent",
            "sub-2",
            "--task",
            "retry",
            "--tokens_in",
            "7",
        ],
        Some("main"),
    );
    assert!(
        spend_other.status.success(),
        "{}",
        String::from_utf8_lossy(&spend_other.stderr)
    );
    let ledger = run(&dir, &["ledger"], Some("main"));
    let text = String::from_utf8(ledger.stdout).unwrap();
    assert!(
        text.contains("\"agents\":[\"main\",\"sub-2\"]") || text.contains("sub-2"),
        "LIBELLUS_AGENT must not narrow the ledger: {text}"
    );
}
