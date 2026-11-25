//! The command line: `libellus <command> --flag value ...`.
//!
//! Commands are the MCP tools without the `libellus_` prefix (note, ls, brief,
//! spend), plus init, check, hook, mcp. Flags come from the same parameter
//! table as the MCP schema. An unknown flag, a flag with no value, or a bad
//! number is an error; nothing is guessed. `LIBELLUS_AGENT` fills `--agent`
//! when the command requires an agent and the flag is omitted.

use crate::json::Value;
use crate::tool::{Kind, Tool};
use crate::{store::Store, tools, SERVER, VERSION};

const HELP: &str = "libellus: a private notepad and number ledger for AI agents

USAGE
  libellus init                       create .libellus/ in this folder
  libellus note --agent A --kind K --summary S [--body B] [--id I] [--status open|resolved]
                [--evidence tool:ref]... [--tags T]...
  libellus get --agent A --id I
  libellus resolve --agent A --id I --outcome TEXT
  libellus ls --agent A [--kind K] [--status S] [--limit N] [--offset N] [--before_id ID] [--bodies]
  libellus brief --agent A [--max_items N] [--host text|cursor]
                                              print the compaction brief as text
  libellus promote --agent A --id I --scope SCOPE
  libellus spend --agent A --task T [--step S] [--model M] [--tokens_in N] [--tokens_out N]
                 [--cost_micro_usd N] [--note TEXT]
  libellus ledger [--agent A] [--task T] [--step S]
  libellus hook --agent NAME [--host cursor|claude|generic]
  libellus check                      read every note and ledger; fail on any damage
  libellus mcp                        start the stdio MCP server (same as libellus-mcp)
  libellus --version

LIBELLUS_AGENT supplies --agent when a command requires one and the flag is omitted.
libellus ledger does not use it: with no --agent it sums every agent.

Output is JSON; the exit code is 1 on any error, 2 on a usage error.
Flags accept --name value and --name=value. A value that starts with -- is
written --name -- --value.";

pub fn run(args: &[String], as_mcp: bool) -> i32 {
    if as_mcp {
        return crate::mcp::serve(&SERVER);
    }
    let Some(cmd) = args.first() else {
        eprintln!("{HELP}");
        return 2;
    };
    match cmd.as_str() {
        "--version" | "-V" | "version" => write_out(&format!(
            "{{\"name\":\"libellus\",\"version\":\"{VERSION}\"}}\n"
        )),
        "--help" | "-h" | "help" => write_out(&format!("{HELP}\n")),
        "mcp" | "serve-mcp" => crate::mcp::serve(&SERVER),
        "init" => finish(
            Store::init()
                .map(|s| Value::obj(vec![("store", Value::String(s.dir.display().to_string()))])),
        ),
        "check" => finish(crate::check::run()),
        "hook" => match crate::hook::from_argv(&args[1..]) {
            Ok(text) => write_out(&text),
            Err(e) => {
                eprintln!("error: {e}");
                2
            }
        },
        other => match tools::TOOLS
            .iter()
            .find(|t| t.name.strip_prefix("libellus_") == Some(other))
        {
            None => {
                eprintln!("unknown command \"{other}\"\n\n{HELP}");
                2
            }
            Some(tool) => {
                let (argv, host) = if other == "brief" {
                    match split_brief_host(&args[1..]) {
                        Ok(pair) => pair,
                        Err(e) => {
                            eprintln!("error: {e}");
                            return 2;
                        }
                    }
                } else {
                    (args[1..].to_vec(), None)
                };
                match build_args(tool, &argv) {
                    Err(e) => {
                        eprintln!("error: {e}");
                        2
                    }
                    Ok(v) => {
                        let v = crate::tool::fill_agent_default(tool, v);
                        let out = tool.validate(&v).and_then(|a| (tool.run)(&a));
                        if other == "brief" {
                            if let Ok(value) = &out {
                                let text = value.get("text").and_then(Value::as_str).unwrap_or("");
                                if host.as_deref() == Some("cursor") {
                                    return write_out(&format!(
                                        "{}\n",
                                        Value::obj(vec![(
                                            "additional_context",
                                            Value::String(text.to_string()),
                                        )])
                                        .to_json()
                                    ));
                                }
                                return write_out(text);
                            }
                        }
                        finish(out)
                    }
                }
            }
        },
    }
}

fn finish(r: Result<Value, String>) -> i32 {
    match r {
        Ok(v) => write_out(&format!("{}\n", v.to_json())),
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

/// Write to stdout. A closed pipe (EPIPE), as in `libellus get | head`, exits 0
/// instead of panicking.
fn write_out(text: &str) -> i32 {
    let mut out = std::io::stdout().lock();
    match std::io::Write::write_all(&mut out, text.as_bytes()) {
        Ok(()) => match std::io::Write::flush(&mut out) {
            Ok(()) => 0,
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => 0,
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn build_args(tool: &Tool, argv: &[String]) -> Result<Value, String> {
    let mut pairs: Vec<(String, Value)> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        if argv[i] == "--" {
            return Err("\"--\" must sit between a flag and a value that starts with --".into());
        }
        let (name, inline) = split_flag(&argv[i])?;
        let flag = name.replace('-', "_");
        let Some(param) = tool.params.iter().find(|p| p.name == flag) else {
            return Err(format!("unknown flag --{}", flag.replace('_', "-")));
        };
        let (value, step) = flag_value(argv, i, param.kind, inline)?;
        let shown = argv[i]
            .trim_start_matches("--")
            .split('=')
            .next()
            .unwrap_or(&flag);
        match (param.kind, value) {
            (Kind::Bool, None) => {
                pairs.push((flag, Value::Bool(true)));
            }
            (Kind::Bool, Some(v)) if v == "true" || v == "false" => {
                pairs.push((flag, Value::Bool(v == "true")));
            }
            (Kind::Bool, Some(v)) => {
                return Err(format!("--{shown} must be true or false, got \"{v}\""));
            }
            (_, None) => return Err(format!("--{shown} needs a value")),
            (Kind::UInt, Some(v)) => {
                let n: u64 = v
                    .parse()
                    .map_err(|_| format!("--{shown} must be a whole number, got \"{v}\""))?;
                pairs.push((flag, Value::uint(n)));
            }
            (Kind::StrList, Some(v)) => match pairs.iter_mut().find(|(k, _)| *k == flag) {
                Some((_, Value::Array(items))) => items.push(Value::str(v.as_str())),
                _ => pairs.push((flag, Value::Array(vec![Value::str(v.as_str())]))),
            },
            (Kind::Str, Some(v)) => {
                if pairs.iter().any(|(k, _)| *k == flag) {
                    return Err(format!("--{shown} given twice"));
                }
                pairs.push((flag, Value::str(v.as_str())));
            }
        }
        i += step;
    }
    Ok(Value::Object(pairs))
}

/// `--name`, `--name=value`, or `--name -- value`. `step` is how many argv
/// entries this flag consumed, including itself.
fn flag_value(
    argv: &[String],
    i: usize,
    kind: Kind,
    inline: Option<&str>,
) -> Result<(Option<String>, usize), String> {
    if let Some(v) = inline {
        return Ok((Some(v.to_string()), 1));
    }
    if kind == Kind::Bool {
        return Ok((None, 1));
    }
    match argv.get(i + 1).map(String::as_str) {
        Some("--") => {
            let value = argv.get(i + 2).ok_or_else(|| {
                format!(
                    "--{} needs a value after --",
                    argv[i].trim_start_matches("--")
                )
            })?;
            Ok((Some(value.clone()), 3))
        }
        Some(v) if !v.starts_with("--") => Ok((Some(v.to_string()), 2)),
        _ => Ok((None, 1)),
    }
}

fn split_flag(token: &str) -> Result<(&str, Option<&str>), String> {
    let rest = token
        .strip_prefix("--")
        .ok_or_else(|| format!("unexpected argument \"{token}\"; flags look like --name value"))?;
    if rest.is_empty() {
        return Err("unexpected argument \"--\"".into());
    }
    if let Some((name, value)) = rest.split_once('=') {
        if name.is_empty() {
            return Err(format!("unknown flag --={value}"));
        }
        Ok((name, Some(value)))
    } else {
        Ok((rest, None))
    }
}

/// `--host` is a CLI presentation flag for `brief`, not an MCP argument.
fn split_brief_host(argv: &[String]) -> Result<(Vec<String>, Option<String>), String> {
    let mut rest = Vec::new();
    let mut host = None;
    let mut i = 0;
    while i < argv.len() {
        if argv[i] == "--host" || argv[i].starts_with("--host=") {
            let (value, step) = if let Some(v) = argv[i].strip_prefix("--host=") {
                (v.to_string(), 1)
            } else if argv.get(i + 1).is_some_and(|v| v == "--") {
                let v = argv
                    .get(i + 2)
                    .ok_or_else(|| "--host needs a value after --".to_string())?;
                (v.clone(), 3)
            } else {
                let v = argv
                    .get(i + 1)
                    .filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| "--host needs a value".to_string())?;
                (v.clone(), 2)
            };
            if value != "text" && value != "cursor" {
                return Err(format!("--host must be text or cursor, got \"{value}\""));
            }
            if host.is_some() {
                return Err("--host given twice".into());
            }
            host = Some(value);
            i += step;
        } else {
            rest.push(argv[i].clone());
            i += 1;
        }
    }
    Ok((rest, host))
}
