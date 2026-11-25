//! Tool groups, chosen in the project's AGENTS.md `agent-tools` block, or by
//! `LIBELLUS_TOOLS` and `LIBELLUS_DISABLE`.
//!
//! ```text
//! libellus: disable ledger
//! libellus: tools core, ledger
//! ```
//!
//! If either environment variable is set (after trimming), it replaces the
//! `agent-tools` line. `LIBELLUS_TOOLS` is the allowlist and `LIBELLUS_DISABLE`
//! is applied after it. `core` is always on. A mistake is never silently
//! ignored: the server falls back to `core` only (the smallest surface) and
//! says why in its instructions and in `tools/list`.

use crate::tool::Tool;
use std::path::Path;

pub struct Enabled {
    pub tools: Vec<&'static Tool>,
    pub notice: Option<String>,
    gate: Gate,
}

enum Gate {
    /// The AGENTS.md line chose the set, or nothing did.
    File,
    /// Environment variables chose the set and the file was not consulted.
    Env {
        tools: Option<Vec<String>>,
        disable: Option<Vec<String>>,
    },
}

impl Enabled {
    /// Why a call to a tool that is not in `tools` failed.
    pub fn refusal(&self, tool: &Tool) -> String {
        if let Some(notice) = &self.notice {
            return format!("tool {} is switched off. {notice}", tool.name);
        }
        match &self.gate {
            Gate::File => format!(
                "tool {} is switched off by the agent-tools block in AGENTS.md; change that line to use it",
                tool.name
            ),
            Gate::Env { tools, disable } => {
                if disable
                    .as_ref()
                    .is_some_and(|g| g.iter().any(|name| name == tool.group))
                {
                    format!(
                        "tool {} is switched off by LIBELLUS_DISABLE; remove \"{}\" from it to use it",
                        tool.name, tool.group
                    )
                } else if tools.is_some() {
                    format!(
                        "tool {} is switched off by LIBELLUS_TOOLS; add \"{}\" to it to use it",
                        tool.name, tool.group
                    )
                } else {
                    format!("tool {} is switched off", tool.name)
                }
            }
        }
    }
}

pub fn enabled(
    all: &'static [Tool],
    name: &str,
    known: &[&str],
    project: Option<&Path>,
) -> Enabled {
    let tools_env = env_list("LIBELLUS_TOOLS");
    let disable_env = env_list("LIBELLUS_DISABLE");
    if tools_env.is_some() || disable_env.is_some() {
        return from_env(all, known, tools_env, disable_env);
    }
    from_file(all, name, known, project)
}

fn env_list(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn fallback(all: &'static [Tool], why: String) -> Enabled {
    Enabled {
        tools: all.iter().filter(|t| t.group == "core").collect(),
        notice: Some(why),
        gate: Gate::File,
    }
}

fn from_env(
    all: &'static [Tool],
    known: &[&str],
    tools: Option<String>,
    disable: Option<String>,
) -> Enabled {
    let parsed = |var: &str, raw: Option<String>| -> Result<Option<Vec<String>>, String> {
        let Some(raw) = raw else {
            return Ok(None);
        };
        Ok(Some(parse_groups(var, &raw, known)?))
    };
    let tools = match parsed("LIBELLUS_TOOLS", tools) {
        Ok(v) => v,
        Err(why) => return fallback(all, why),
    };
    let disable = match parsed("LIBELLUS_DISABLE", disable) {
        Ok(v) => v,
        Err(why) => return fallback(all, why),
    };
    Enabled {
        tools: pick(all, tools.as_deref(), disable.as_deref()),
        notice: None,
        gate: Gate::Env { tools, disable },
    }
}

fn parse_groups(var: &str, raw: &str, known: &[&str]) -> Result<Vec<String>, String> {
    let groups: Vec<String> = raw
        .split(',')
        .map(|g| g.trim().to_lowercase())
        .filter(|g| !g.is_empty())
        .collect();
    if groups.is_empty() {
        return Err(format!(
            "{var} ignored, only core tools are on: names no groups"
        ));
    }
    if let Some(bad) = groups.iter().find(|g| !known.contains(&g.as_str())) {
        return Err(format!(
            "{var} ignored, only core tools are on: unknown group \"{bad}\" (groups: {})",
            known.join(", ")
        ));
    }
    if var == "LIBELLUS_DISABLE" && groups.iter().any(|g| g == "core") {
        return Err(format!(
            "{var} ignored, only core tools are on: `core` cannot be disabled"
        ));
    }
    Ok(groups)
}

fn pick(
    all: &'static [Tool],
    tools: Option<&[String]>,
    disable: Option<&[String]>,
) -> Vec<&'static Tool> {
    all.iter()
        .filter(|t| {
            if t.group == "core" {
                return true;
            }
            let allowed = tools
                .map(|g| g.iter().any(|name| name == t.group))
                .unwrap_or(true);
            let disabled = disable
                .map(|g| g.iter().any(|name| name == t.group))
                .unwrap_or(false);
            allowed && !disabled
        })
        .collect()
}

fn from_file(all: &'static [Tool], name: &str, known: &[&str], project: Option<&Path>) -> Enabled {
    let core_only = |why: String| {
        fallback(
            all,
            format!("agent-tools line ignored, only core tools are on: {why}"),
        )
    };
    let open = |tools: Vec<&'static Tool>| Enabled {
        tools,
        notice: None,
        gate: Gate::File,
    };
    let Some(root) = project else {
        return open(all.iter().collect());
    };
    let path = root.join("AGENTS.md");
    let text = match std::fs::metadata(&path) {
        Err(_) => {
            return open(all.iter().collect());
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
        return open(all.iter().collect());
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
    open(all.iter().filter(|t| on(t.group)).collect())
}
