//! The command line: `libellus <command> --flag value ...`.
//!
//! Commands are the MCP tools without the `libellus_` prefix (note, ls, brief,
//! spend), plus init, check, mcp. Flags come from the same parameter table as
//! the MCP schema. An unknown flag, a flag with no value, or a bad number is
//! an error; nothing is guessed.

use crate::json::Value;
use crate::tool::{Kind, Tool};
use crate::{fsx, ledger, note, store::Store, tools, SERVER, VERSION};

const HELP: &str = "libellus: a private notepad and number ledger for AI agents

USAGE
  libellus init                       create .libellus/ in this folder
  libellus note --agent A --kind K --summary S [--body B] [--id I] [--status open|resolved]
                [--evidence tool:ref]... [--tags T]...
  libellus get --agent A --id I
  libellus resolve --agent A --id I --outcome TEXT
  libellus ls --agent A [--kind K] [--status S] [--limit N] [--bodies]
  libellus brief --agent A [--max_items N]       print the compaction brief as text
  libellus spend --agent A --task T [--step S] [--model M] [--tokens_in N] [--tokens_out N]
                 [--cost_micro_usd N] [--note TEXT]
  libellus check                      read every note and ledger; fail on any damage
  libellus mcp                        start the stdio MCP server (same as libellus-mcp)
  libellus --version

Output is JSON; the exit code is 1 on any error, 2 on a usage error.";

pub fn run(args: &[String], as_mcp: bool) -> i32 {
    if as_mcp {
        return crate::mcp::serve(&SERVER);
    }
    let Some(cmd) = args.first() else {
        eprintln!("{HELP}");
        return 2;
    };
    match cmd.as_str() {
        "--version" | "-V" | "version" => {
            println!("{{\"name\":\"libellus\",\"version\":\"{VERSION}\"}}");
            0
        }
        "--help" | "-h" | "help" => {
            println!("{HELP}");
            0
        }
        "mcp" | "serve-mcp" => crate::mcp::serve(&SERVER),
        "init" => finish(
            Store::init()
                .map(|s| Value::obj(vec![("store", Value::String(s.dir.display().to_string()))])),
        ),
        "check" => finish(check()),
        other => match tools::TOOLS
            .iter()
            .find(|t| t.name.strip_prefix("libellus_") == Some(other))
        {
            None => {
                eprintln!("unknown command \"{other}\"\n\n{HELP}");
                2
            }
            Some(tool) => match build_args(tool, &args[1..]) {
                Err(e) => {
                    eprintln!("{e}");
                    2
                }
                Ok(v) => {
                    let out = tool.validate(&v).and_then(|a| (tool.run)(&a));
                    if other == "brief" {
                        if let Ok(Value::Object(_)) = &out {
                            let text = out
                                .as_ref()
                                .ok()
                                .and_then(|o| o.get("text"))
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            print!("{text}");
                            return 0;
                        }
                    }
                    finish(out)
                }
            },
        },
    }
}

fn finish(r: Result<Value, String>) -> i32 {
    match r {
        Ok(v) => {
            println!("{}", v.to_json());
            0
        }
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
        let flag = argv[i].strip_prefix("--").ok_or_else(|| {
            format!(
                "unexpected argument \"{}\"; flags look like --name value",
                argv[i]
            )
        })?;
        let flag = flag.replace('-', "_");
        let param = tool.params.iter().find(|p| p.name == flag);
        let kind = param.map_or(Kind::Str, |p| p.kind);
        let value = argv.get(i + 1).filter(|v| !v.starts_with("--"));
        match (kind, value) {
            (Kind::Bool, Some(v)) if v == "true" || v == "false" => {
                pairs.push((flag, Value::Bool(v == "true")));
                i += 2;
            }
            (Kind::Bool, _) => {
                pairs.push((flag, Value::Bool(true)));
                i += 1;
            }
            (_, None) => {
                return Err(format!(
                    "--{} needs a value",
                    argv[i].trim_start_matches("--")
                ))
            }
            (Kind::UInt, Some(v)) => {
                let n: u64 = v.parse().map_err(|_| {
                    format!(
                        "--{} must be a whole number, got \"{v}\"",
                        argv[i].trim_start_matches("--")
                    )
                })?;
                pairs.push((flag, Value::uint(n)));
                i += 2;
            }
            (Kind::StrList, Some(v)) => {
                match pairs.iter_mut().find(|(k, _)| *k == flag) {
                    Some((_, Value::Array(items))) => items.push(Value::str(v.as_str())),
                    _ => pairs.push((flag, Value::Array(vec![Value::str(v.as_str())]))),
                }
                i += 2;
            }
            (Kind::Str, Some(v)) => {
                if pairs.iter().any(|(k, _)| *k == flag) {
                    return Err(format!(
                        "--{} given twice",
                        argv[i].trim_start_matches("--")
                    ));
                }
                pairs.push((flag, Value::str(v.as_str())));
                i += 2;
            }
        }
    }
    Ok(Value::Object(pairs))
}

/// Read every note and ledger. Any damage is an error; stray temp files are
/// reported so they can be removed.
fn check() -> Result<Value, String> {
    let store = Store::open()?;
    let mut agents = Vec::new();
    let mut strays = Vec::new();
    for agent in store.agents()? {
        let notes = note::load_all(&store, &agent)?;
        let entries = ledger::read(&store, &agent)?;
        agents.push(Value::obj(vec![
            ("agent", Value::str(&agent)),
            ("notes", Value::uint(notes.len() as u64)),
            ("ledger_entries", Value::uint(entries.len() as u64)),
        ]));
        find_temps(&store.agent_dir(&agent)?, &mut strays);
    }
    Ok(Value::obj(vec![
        ("ok", Value::Bool(true)),
        ("agents", Value::Array(agents)),
        (
            "stray_temp_files",
            Value::Array(
                strays
                    .iter()
                    .map(|s| Value::str(fsx::clip(s, 300)))
                    .collect(),
            ),
        ),
    ]))
}

fn find_temps(dir: &std::path::Path, out: &mut Vec<String>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                find_temps(&p, out);
            } else if p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.') && n.contains(".tmp-"))
            {
                out.push(p.display().to_string());
            }
        }
    }
}
