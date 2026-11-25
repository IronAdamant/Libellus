//! Print a host hook that runs `libellus brief` and puts the text back into
//! the session. The snippet is text for a person to paste. Libellus does not
//! install it and does not run a background process.

use crate::fsx;
use crate::json::Value;
use crate::tool::{Kind, Param};

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
        None => std::env::var("LIBELLUS_AGENT")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "missing --agent (or set LIBELLUS_AGENT)".to_string())?,
    };
    if !fsx::valid_slug(&agent) || fsx::reserved_name(&agent) {
        return Err(format!(
            "invalid agent \"{}\": use lowercase letters, digits and '-'",
            fsx::clip(&agent, 40)
        ));
    }
    snippet(&agent, &host)
}

fn snippet(agent: &str, host: &str) -> Result<String, String> {
    let cmd = format!("libellus brief --agent {agent}");
    let text = match host {
        "generic" => format!(
            "# Run this at session start and after compaction, and put the text into the session.\n{cmd}\n"
        ),
        "claude" => format!(
            "# Paste the JSON into .claude/settings.json.\n\
             # Run it at session start and after compaction. SessionStart with the compact\n\
             # matcher runs after compaction, and the command's plain text is added to the session.\n\
             {{\n  \"hooks\": {{\n    \"SessionStart\": [\n      {{\n        \"matcher\": \"startup|resume|clear|compact\",\n        \"hooks\": [\n          {{\n            \"type\": \"command\",\n            \"command\": \"{cmd}\"\n          }}\n        ]\n      }}\n    ]\n  }}\n}}\n"
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
}
