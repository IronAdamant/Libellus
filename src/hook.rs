//! Host hooks. `libellus hook` prints a snippet for a person to paste; it
//! installs nothing. `libellus hook-run` is what a Claude Code hook runs: it
//! reads the hook's JSON on stdin and prints the brief as
//! `hookSpecificOutput.additionalContext`.
//!
//! The Claude snippet uses the exec form (`command` plus `args`), so no shell
//! reads it: the same text works under cmd, PowerShell and Git Bash.

use crate::fsx;
use crate::json::{self, Value};
use crate::tool::{Kind, Param};
use std::io::Read;

/// The flags `libellus hook` takes. They go through the same parser as the
/// tool commands.
const PARAMS: &[Param] = &[
    Param {
        name: "agent",
        kind: Kind::Str,
        required: false,
        doc: "The agent whose brief the hook prints.",
        choices: &[],
    },
    Param {
        name: "host",
        kind: Kind::Str,
        required: false,
        doc: "cursor, claude or generic.",
        choices: &[],
    },
];

fn env_agent() -> Option<String> {
    std::env::var("LIBELLUS_AGENT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn check_agent(agent: &str) -> Result<(), String> {
    if !fsx::valid_slug(agent) || fsx::reserved_name(agent) {
        return Err(format!(
            "invalid agent \"{}\": use lowercase letters, digits and '-'",
            fsx::clip(agent, 40)
        ));
    }
    Ok(())
}

pub fn from_argv(argv: &[String]) -> Result<String, String> {
    let parsed = crate::cli::parse_flags(PARAMS, argv)
        .map_err(|e| format!("{e} (hook takes --agent and --host)"))?;
    let agent = parsed
        .get("agent")
        .and_then(Value::as_str)
        .map(String::from);
    let host = parsed
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or("generic")
        .to_string();
    let agent = match agent {
        Some(name) => name,
        None => env_agent().ok_or_else(|| "missing --agent (or set LIBELLUS_AGENT)".to_string())?,
    };
    check_agent(&agent)?;
    snippet(&agent, &host)
}

/// The JSON for `.claude/settings.json`: SessionStart at startup, resume and
/// after compaction, and SubagentStart for every sub-agent.
pub fn claude_json(agent: &str) -> Value {
    let exec = |args: Vec<&str>| {
        Value::obj(vec![
            ("type", Value::str("command")),
            ("command", Value::str("libellus")),
            (
                "args",
                Value::Array(args.into_iter().map(Value::str).collect()),
            ),
        ])
    };
    Value::obj(vec![(
        "hooks",
        Value::obj(vec![
            (
                "SessionStart",
                Value::Array(vec![Value::obj(vec![
                    ("matcher", Value::str("startup|resume|compact")),
                    (
                        "hooks",
                        Value::Array(vec![exec(vec![
                            "hook-run",
                            "session-start",
                            "--agent",
                            agent,
                        ])]),
                    ),
                ])]),
            ),
            (
                "SubagentStart",
                Value::Array(vec![Value::obj(vec![(
                    "hooks",
                    Value::Array(vec![exec(vec!["hook-run", "subagent-start"])]),
                )])]),
            ),
        ]),
    )])
}

fn snippet(agent: &str, host: &str) -> Result<String, String> {
    let cmd = format!("libellus brief --agent {agent}");
    let text = match host {
        "generic" => format!(
            "# Run this at session start and after compaction, and put the text into the session.\n{cmd}\n"
        ),
        "claude" => format!(
            "# Paste the JSON into .claude/settings.json (merge it into an existing \"hooks\" object).\n\
             # SessionStart runs at startup, on resume and after compaction, and adds the same\n\
             # text as `{cmd}` to the session.\n\
             # SubagentStart gives each sub-agent its own Libellus name (<agent_type>-<8 characters\n\
             # of agent_id>) and its brief, including its board tasks, before its first prompt.\n\
             # The exec form (command plus args) runs without a shell, so it works the same on\n\
             # Windows, macOS and Linux. libellus must be on PATH.\n\
             {}\n",
            pretty(&claude_json(agent), 0)
        ),
        "cursor" => format!(
            "# Paste the JSON into .cursor/hooks.json.\n\
             # sessionStart runs when a chat starts. The command prints {{\"additional_context\": ...}}\n\
             # and Cursor adds that text to the session.\n\
             # preCompact cannot put text back into the model. After compaction, run `{cmd}`\n\
             # again (or call libellus_brief) and put the text into the session yourself.\n\
             {{\n  \"version\": 1,\n  \"hooks\": {{\n    \"sessionStart\": [\n      {{ \"command\": \"{cmd} --host cursor\" }}\n    ]\n  }}\n}}\n"
        ),
        other => {
            return Err(format!(
                "unknown host \"{other}\" (use cursor, claude or generic)"
            ))
        }
    };
    Ok(text)
}

/// Indented JSON for a person to read and paste.
fn pretty(v: &Value, depth: usize) -> String {
    let pad = "  ".repeat(depth + 1);
    let end = "  ".repeat(depth);
    match v {
        Value::Object(pairs) if !pairs.is_empty() => {
            let items: Vec<String> = pairs
                .iter()
                .map(|(k, v)| {
                    let mut key = String::new();
                    json::write_string(k, &mut key);
                    format!("{pad}{key}: {}", pretty(v, depth + 1))
                })
                .collect();
            format!("{{\n{}\n{end}}}", items.join(",\n"))
        }
        Value::Array(items) if items.iter().any(|i| matches!(i, Value::Object(_))) => {
            let items: Vec<String> = items
                .iter()
                .map(|i| format!("{pad}{}", pretty(i, depth + 1)))
                .collect();
            format!("[\n{}\n{end}]", items.join(",\n"))
        }
        other => other.to_json(),
    }
}

/// The Libellus agent name for a Claude Code sub-agent:
/// `<agent_type>-<last 8 letters and digits of agent_id>`, lowercased, with
/// anything that is not a letter or digit turned into `-`. The type part is
/// at most 40 characters and is `sub` when empty.
pub fn subagent_name(agent_type: &str, agent_id: &str) -> String {
    let kind = fsx::slug_from(agent_type, 1, "sub");
    let mut kind: String = kind.chars().take(40).collect();
    while kind.ends_with('-') {
        kind.pop();
    }
    let id = agent_id.strip_prefix("agent-").unwrap_or(agent_id);
    let tail: Vec<char> = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let short: String = tail[tail.len().saturating_sub(8)..].iter().collect();
    if short.is_empty() {
        kind
    } else {
        format!("{kind}-{short}")
    }
}

fn output(event: &str, text: &str) -> String {
    let v = Value::obj(vec![(
        "hookSpecificOutput",
        Value::obj(vec![
            ("hookEventName", Value::str(event)),
            ("additionalContext", Value::str(text)),
        ]),
    )]);
    format!("{}\n", v.to_json())
}

fn brief_text(agent: &str) -> Result<String, String> {
    let tool = crate::tools::TOOLS
        .iter()
        .find(|t| t.name == "libellus_brief")
        .ok_or("no brief tool")?;
    let args = tool.validate(&Value::obj(vec![("agent", Value::str(agent))]))?;
    let v = (tool.run)(&args)?;
    Ok(v.get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string())
}

/// What a hook-run prints, from its arguments and the hook's stdin. `Err`
/// is a message for stderr; the hook still exits 0 so a broken store or bad
/// input never stops the session.
pub fn hook_output(argv: &[String], stdin: &str) -> Result<String, String> {
    let Some(event) = argv.first() else {
        return Err("hook-run needs session-start or subagent-start".into());
    };
    match event.as_str() {
        "session-start" => {
            let parsed = crate::cli::parse_flags(&PARAMS[..1], &argv[1..])?;
            let agent = match parsed.get("agent").and_then(Value::as_str) {
                Some(a) => a.to_string(),
                None => env_agent().ok_or("hook-run session-start needs --agent")?,
            };
            check_agent(&agent)?;
            Ok(output("SessionStart", &brief_text(&agent)?))
        }
        "subagent-start" => {
            if argv.len() > 1 {
                return Err("hook-run subagent-start takes no flags".into());
            }
            let input =
                json::parse(stdin.trim()).map_err(|e| format!("hook input is not JSON: {e}"))?;
            if !matches!(input, Value::Object(_)) {
                return Err("hook input is not a JSON object".into());
            }
            let agent_type = input
                .get("agent_type")
                .and_then(Value::as_str)
                .unwrap_or("");
            let agent_id = input.get("agent_id").and_then(Value::as_str).unwrap_or("");
            if agent_type.is_empty() && agent_id.is_empty() {
                return Err("hook input has no agent_type or agent_id".into());
            }
            let name = subagent_name(agent_type, agent_id);
            let head = format!(
                "Your Libellus agent name is {name}. Pass agent \"{name}\" to every libellus_* call, unless the prompt that started you names another Libellus agent (such as sub-2); then use that name and call libellus_brief with it first. Tasks for you are on the shared board: libellus_board, and tick them off with libellus_plan.\n\n"
            );
            let brief = brief_text(&name)?;
            Ok(output("SubagentStart", &format!("{head}{brief}")))
        }
        other => Err(format!(
            "unknown hook \"{}\" (use session-start or subagent-start)",
            fsx::clip(other, 40)
        )),
    }
}

/// `libellus hook-run ...`. Always exits 0.
pub fn run(argv: &[String]) -> i32 {
    let mut input = String::new();
    let wants_stdin = argv.first().is_some_and(|e| e == "subagent-start");
    if wants_stdin {
        let mut limited = std::io::stdin().lock().take(1_048_576);
        if let Err(e) = limited.read_to_string(&mut input) {
            eprintln!("libellus hook-run: cannot read the hook input: {e}");
            return 0;
        }
    }
    match hook_output(argv, &input) {
        Ok(text) => {
            let mut out = std::io::stdout().lock();
            let _ = std::io::Write::write_all(&mut out, text.as_bytes());
            let _ = std::io::Write::flush(&mut out);
        }
        Err(e) => eprintln!("libellus hook-run: {e}"),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn snippets_name_the_brief_command() {
        let text = from_argv(&args(&["--agent", "main", "--host", "generic"])).unwrap();
        assert!(text.contains("libellus brief --agent main\n"), "{text}");
        assert!(text.contains("session start") && text.contains("after compaction"));
        let claude = from_argv(&args(&["--agent", "main", "--host", "claude"])).unwrap();
        assert!(claude.contains("libellus brief --agent main"));
        assert!(claude.contains("after compaction"));
        let cursor = from_argv(&args(&["--agent", "main", "--host", "cursor"])).unwrap();
        assert!(cursor.contains("libellus brief --agent main --host cursor"));
        assert!(cursor.contains("preCompact cannot put text back"));
        assert!(from_argv(&args(&["--agent", "Main"])).is_err());
        assert!(from_argv(&args(&["--agent", "main", "--host", "nope"])).is_err());
        assert!(from_argv(&args(&["--colour", "red"])).is_err());
        assert!(from_argv(&args(&["--agent"])).is_err());
    }

    #[test]
    fn subagent_names_are_slugs() {
        assert_eq!(subagent_name("Explore", "agent-abc123"), "explore-abc123");
        assert_eq!(
            subagent_name("code-reviewer", "agent-0123456789abcdef"),
            "code-reviewer-89abcdef"
        );
        assert_eq!(subagent_name("", "XYZ"), "sub-xyz");
        assert_eq!(subagent_name("CON", ""), "con-sub");
        let long = subagent_name(&"a".repeat(90), "agent-1");
        assert!(fsx::valid_slug(&long), "{long}");
    }
}
