//! Ledger totals, and tool groups chosen by the environment.
mod common;
use common::*;
use std::fs;
use std::process::Command;

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(CLI)
        .args(args)
        .current_dir(dir)
        .env_remove("LIBELLUS_DIR")
        .env_remove("LIBELLUS_TOOLS")
        .env_remove("LIBELLUS_DISABLE")
        .output()
        .unwrap()
}

fn spend(dir: &std::path::Path, agent: &str, task: &str, step: Option<&str>, tokens_in: &str) {
    let mut args = vec![
        "spend",
        "--agent",
        agent,
        "--task",
        task,
        "--tokens_in",
        tokens_in,
        "--tokens_out",
        "1",
        "--cost_micro_usd",
        "10",
    ];
    if let Some(step) = step {
        args.extend(["--step", step]);
    }
    let out = cli(dir, &args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn ledger_filters_by_task_and_breaks_down_by_step() {
    let dir = project("ledger-task");
    init(&dir);
    spend(&dir, "main", "retry", Some("plan"), "100");
    spend(&dir, "main", "retry", None, "10");
    spend(&dir, "main", "other", Some("plan"), "5");
    let out = cli(&dir, &["ledger", "--agent", "main", "--task", "retry"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("\"entries\":2")
            && text.contains("\"tokens_in\":110")
            && text.contains("\"tokens_out\":2")
            && text.contains("\"cost_micro_usd\":20")
            && !text.contains("by_task")
            && text.contains("\"by_step\"")
            && text.contains("\"plan\":{\"entries\":1,\"tokens_in\":100")
            && text.contains("\"\":{\"entries\":1,\"tokens_in\":10"),
        "{text}"
    );
    assert!(!text.contains("\"tokens_in\":115"), "{text}");

    let all = cli(&dir, &["ledger", "--agent", "main"]);
    let all = String::from_utf8(all.stdout).unwrap();
    assert!(
        all.contains("\"entries\":3")
            && all.contains("\"tokens_in\":115")
            && all.contains("\"by_task\"")
            && all.contains("\"retry\":{\"entries\":2,\"tokens_in\":110")
            && all.contains("\"other\":{\"entries\":1,\"tokens_in\":5"),
        "{all}"
    );

    let step = cli(&dir, &["ledger", "--agent", "main", "--step", "plan"]);
    let step = String::from_utf8(step.stdout).unwrap();
    assert!(
        step.contains("\"entries\":2")
            && step.contains("\"tokens_in\":105")
            && step.contains("\"by_task\"")
            && !step.contains("by_step"),
        "{step}"
    );
}

#[test]
fn ledger_sums_every_agent_and_a_damaged_file_stays_damaged() {
    let dir = project("ledger-all");
    init(&dir);
    spend(&dir, "main", "retry", None, "4");
    spend(&dir, "sub-2", "retry", None, "6");
    let out = cli(&dir, &["ledger"]);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("\"agents\":[\"main\",\"sub-2\"]") && text.contains("\"tokens_in\":10"),
        "{text}"
    );

    let path = dir.join(".libellus/agents/main/ledger.jsonl");
    let mut damaged = fs::read_to_string(&path).unwrap();
    damaged.push_str("{not json\n");
    fs::write(&path, &damaged).unwrap();
    let out = cli(&dir, &["ledger"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("corrupt ledger") && err.contains("line 2"),
        "{err}"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), damaged);

    let huge = format!(
        "{{\"ts\":\"2026-11-25T00:00:00Z\",\"agent\":\"sub-2\",\"task\":\"retry\",\"tokens_in\":{},\"tokens_out\":0,\"cost_micro_usd\":0}}\n{{\"ts\":\"2026-11-25T00:00:01Z\",\"agent\":\"sub-2\",\"task\":\"retry\",\"tokens_in\":1,\"tokens_out\":0,\"cost_micro_usd\":0}}\n",
        u64::MAX
    );
    let other = dir.join(".libellus/agents/sub-2/ledger.jsonl");
    fs::write(&other, &huge).unwrap();
    // main is still damaged, so the all-agent read fails there first.
    fs::write(&path, "").unwrap();
    let out = cli(&dir, &["ledger", "--agent", "sub-2"]);
    let body = String::from_utf8(out.stdout).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        body.contains("ledger_error") && body.contains("line 2"),
        "{body}"
    );
    assert_eq!(fs::read_to_string(&other).unwrap(), huge);

    let bad = cli(&dir, &["ledger", "--agent", "main", "--task", "../x"]);
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("invalid task"));
    let bad = cli(&dir, &["ledger", "--agent", "Main"]);
    assert_eq!(bad.status.code(), Some(1));
    let bad = cli(&dir, &["ledger", "--colour", "red"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown flag"));
}

#[test]
fn env_groups_win_over_agents_md_and_a_typo_falls_back_to_core() {
    let dir = project("env-groups");
    init(&dir);
    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nlibellus: tools core, ledger\n```\n",
    )
    .unwrap();
    let on = talk_env(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
        &[],
    );
    assert!(
        on[0].contains("libellus_spend") && on[0].contains("libellus_ledger"),
        "{}",
        on[0]
    );

    let off = talk_env(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"initialize\",\"params\":{}}\n",
        &[("LIBELLUS_DISABLE", "ledger")],
    );
    assert!(
        !off[0].contains("libellus_spend")
            && !off[0].contains("libellus_ledger")
            && off[0].contains("libellus_note")
            && off[0].contains("libellus_get")
            && !off[0].contains("NOTICE"),
        "{}",
        off[0]
    );
    let denied = talk_env(
        &dir,
        call(
            "libellus_spend",
            r#"{"agent":"main","task":"t","tokens_in":1}"#,
            1,
        )
        .as_bytes(),
        &[("LIBELLUS_DISABLE", "ledger")],
    );
    assert!(
        is_error(&denied[0]) && denied[0].contains("LIBELLUS_DISABLE"),
        "{}",
        denied[0]
    );
    let denied = talk_env(
        &dir,
        call("libellus_ledger", r#"{"agent":"main"}"#, 1).as_bytes(),
        &[("LIBELLUS_DISABLE", "Ledger")],
    );
    assert!(
        is_error(&denied[0]) && denied[0].contains("LIBELLUS_DISABLE"),
        "{}",
        denied[0]
    );

    let typo = talk_env(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"initialize\",\"params\":{}}\n",
        &[("LIBELLUS_DISABLE", "ledgr")],
    );
    assert!(
        typo[0].contains("NOTICE")
            && typo[0].contains("ledgr")
            && typo[0].contains("libellus_note")
            && !typo[0].contains("libellus_spend")
            && !typo[0].contains("libellus_ledger"),
        "{}",
        typo[0]
    );
    assert!(
        typo[1].contains("ledgr") && typo[1].contains("NOTICE"),
        "{}",
        typo[1]
    );

    // A set variable replaces the file, so an allowlist turns ledger back on
    // even when AGENTS.md would have disabled it.
    fs::write(
        dir.join("AGENTS.md"),
        "```agent-tools\nlibellus: disable ledger\n```\n",
    )
    .unwrap();
    let back = talk_env(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
        &[("LIBELLUS_TOOLS", "core, ledger")],
    );
    assert!(
        back[0].contains("libellus_ledger") && !back[0].contains("NOTICE"),
        "{}",
        back[0]
    );
    let both = talk_env(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
        &[
            ("LIBELLUS_TOOLS", "core,ledger"),
            ("LIBELLUS_DISABLE", "ledger"),
        ],
    );
    assert!(!both[0].contains("libellus_spend"), "{}", both[0]);

    let core = talk_env(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
        &[("LIBELLUS_DISABLE", "core")],
    );
    assert!(
        core[0].contains("cannot be disabled")
            && core[0].contains("libellus_note")
            && !core[0].contains("libellus_spend"),
        "{}",
        core[0]
    );

    // An empty value counts as unset, so the file applies again.
    let empty = talk_env(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n",
        &[("LIBELLUS_DISABLE", "  ")],
    );
    assert!(
        !empty[0].contains("libellus_spend") && !empty[0].contains("NOTICE"),
        "{}",
        empty[0]
    );
}
