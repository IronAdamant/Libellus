//! Tool groups, chosen in the project's AGENTS.md `agent-tools` block.
//!
//! ```text
//! libellus: disable ledger
//! libellus: tools core, ledger
//! ```
//!
//! `core` is always on. A mistake in the line is never silently ignored: the
//! server falls back to `core` only (the smallest surface) and says why in
//! its instructions and in `tools/list`.

use crate::tool::Tool;
use std::path::Path;

pub struct Enabled {
    pub tools: Vec<&'static Tool>,
    pub notice: Option<String>,
}

pub fn enabled(
    all: &'static [Tool],
    name: &str,
    known: &[&str],
    project: Option<&Path>,
) -> Enabled {
    let core_only = |why: String| Enabled {
        tools: all.iter().filter(|t| t.group == "core").collect(),
        notice: Some(format!(
            "agent-tools line ignored, only core tools are on: {why}"
        )),
    };
    let Some(root) = project else {
        return Enabled {
            tools: all.iter().collect(),
            notice: None,
        };
    };
    let path = root.join("AGENTS.md");
    let text = match std::fs::metadata(&path) {
        Err(_) => {
            return Enabled {
                tools: all.iter().collect(),
                notice: None,
            }
        }
        Ok(m) if m.len() > 1_048_576 => return core_only("AGENTS.md is larger than 1 MiB".into()),
        Ok(_) => match crate::fsx::read_strict(&path) {
            Ok(t) => t,
            Err(e) => return core_only(e),
        },
    };
    let mut directives: Vec<String> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            if inside {
                inside = false;
            } else if t.trim_start_matches('`').trim() == "agent-tools" {
                inside = true;
            }
            continue;
        }
        if inside {
            if let Some(rest) = t.strip_prefix(name).and_then(|r| r.strip_prefix(':')) {
                directives.push(rest.trim().to_string());
            }
        }
    }
    if directives.is_empty() {
        return Enabled {
            tools: all.iter().collect(),
            notice: None,
        };
    }
    if directives.len() > 1 {
        return core_only(format!(
            "more than one `{name}:` line in the agent-tools block"
        ));
    }
    let d = &directives[0];
    let (verb, list) = d
        .split_once(char::is_whitespace)
        .unwrap_or((d.as_str(), ""));
    let groups: Vec<&str> = list
        .split(',')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .collect();
    if groups.is_empty() {
        return core_only(format!("`{name}: {d}` names no groups"));
    }
    if let Some(bad) = groups.iter().find(|g| !known.contains(g)) {
        return core_only(format!(
            "unknown group \"{bad}\" (groups: {})",
            known.join(", ")
        ));
    }
    let on = |g: &str| match verb {
        "disable" => g == "core" || !groups.contains(&g),
        "tools" => g == "core" || groups.contains(&g),
        _ => true,
    };
    if verb != "disable" && verb != "tools" {
        return core_only(format!(
            "unknown directive \"{verb}\" (use `disable` or `tools`)"
        ));
    }
    if verb == "disable" && groups.contains(&"core") {
        return core_only("`core` cannot be disabled".into());
    }
    Enabled {
        tools: all.iter().filter(|t| on(t.group)).collect(),
        notice: None,
    }
}
