//! The tool definitions are re-read by the model on every turn, so their size
//! is a cost. These budgets keep `tools/list` and the server instructions
//! from growing back past what 1.3.1 trimmed them to.

mod common;
use common::*;
use libellus::json::{self, Value};

/// Most bytes for the compact JSON of the `tools` array, every group on.
const TOOLS_BUDGET: usize = 8_500;
/// Most bytes for the `instructions` string.
const INSTRUCTIONS_BUDGET: usize = 600;

fn surface(dir: &std::path::Path) -> (Value, String) {
    let replies = talk_env(
        dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
        &[("LIBELLUS_TOOLS", "core, ledger, board")],
    );
    let init = json::parse(&replies[0]).unwrap();
    let instructions = init
        .get("result")
        .and_then(|r| r.get("instructions"))
        .and_then(Value::as_str)
        .unwrap()
        .to_string();
    let listed = json::parse(&replies[1]).unwrap();
    let tools = listed
        .get("result")
        .and_then(|r| r.get("tools"))
        .cloned()
        .unwrap();
    (tools, instructions)
}

#[test]
fn tool_definitions_stay_within_the_byte_budget() {
    let dir = project("surface-budget");
    init(&dir);
    let (tools, instructions) = surface(&dir);
    let names: Vec<&str> = tools
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.get("name").and_then(Value::as_str).unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "libellus_note",
            "libellus_get",
            "libellus_resolve",
            "libellus_ls",
            "libellus_brief",
            "libellus_promote",
            "libellus_spend",
            "libellus_ledger",
            "libellus_doctor",
            "libellus_plan",
            "libellus_board",
        ]
    );
    let bytes = tools.to_json().len();
    assert!(
        bytes <= TOOLS_BUDGET,
        "tools/list is {bytes} bytes, over the {TOOLS_BUDGET} byte budget. Shorten descriptions; move detail to SPEC.md."
    );
    assert!(
        instructions.len() < INSTRUCTIONS_BUDGET,
        "instructions are {} bytes, over the {INSTRUCTIONS_BUDGET} byte budget",
        instructions.len()
    );
}

#[test]
fn the_trimmed_schemas_still_say_what_a_call_needs() {
    let dir = project("surface-shape");
    init(&dir);
    let (tools, _) = surface(&dir);
    let text = tools.to_json();
    // Batches keep their item types, enums, required keys and caps.
    for needle in [
        "\"maxItems\":50",
        "\"maxItems\":100",
        "\"required\":[\"kind\",\"summary\"]",
        "\"required\":[\"id\",\"outcome\"]",
        "\"required\":[\"title\"]",
        "\"enum\":[\"decision\",\"dead-end\",\"question\",\"finding\",\"context\"]",
        "\"enum\":[\"todo\",\"doing\",\"blocked\",\"done\",\"dropped\"]",
        // The outline grammar is in the plan description.
        "[~] doing",
        "[!] blocked",
        "[x] done",
        "[-] dropped",
    ] {
        assert!(text.contains(needle), "missing {needle}: {text}");
    }
    // Every tool says what it is for.
    for t in tools.as_array().unwrap() {
        let d = t.get("description").and_then(Value::as_str).unwrap();
        assert!(d.len() > 20, "{d}");
    }
}

#[test]
fn ledger_archive_replaces_the_archive_tool() {
    let dir = project("surface-archive");
    init(&dir);
    let input = format!(
        "{}{}{}{}",
        call(
            "libellus_spend",
            r#"{"agent":"main","task":"t","tokens_in":3}"#,
            1
        ),
        call(
            "libellus_ledger",
            r#"{"agent":"main","task":"t","archive":true}"#,
            2
        ),
        call("libellus_ledger", r#"{"archive":true}"#, 3),
        call("libellus_ledger", r#"{"agent":"main","archive":true}"#, 4),
    );
    let replies = talk(&dir, input.as_bytes());
    assert!(is_ok(&replies[0]), "{}", replies[0]);
    assert!(
        is_error(&replies[1]) && replies[1].contains("only agent"),
        "{}",
        replies[1]
    );
    assert!(
        is_error(&replies[2]) && replies[2].contains("LIBELLUS_AGENT"),
        "{}",
        replies[2]
    );
    assert!(
        is_ok(&replies[3]) && replies[3].contains("archived"),
        "{}",
        replies[3]
    );
    // LIBELLUS_AGENT fills the agent for archive, and totals start fresh.
    let replies = talk_env(
        &dir,
        format!(
            "{}{}{}",
            call(
                "libellus_spend",
                r#"{"agent":"main","task":"t","tokens_in":5}"#,
                1
            ),
            call("libellus_ledger", r#"{"archive":true}"#, 2),
            call("libellus_archive", r#"{"agent":"main"}"#, 3),
        )
        .as_bytes(),
        &[("LIBELLUS_AGENT", "main")],
    );
    assert!(
        is_ok(&replies[1]) && replies[1].contains("archived"),
        "{}",
        replies[1]
    );
    assert!(
        replies[2].contains("unknown tool: libellus_archive"),
        "{}",
        replies[2]
    );
    let archived = dir
        .join(".libellus/agents/main/archive")
        .read_dir()
        .unwrap()
        .count();
    assert!(archived >= 2, "{archived}");
}
