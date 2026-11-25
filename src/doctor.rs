//! `libellus doctor`: read-only diagnostics. Each check says what it found
//! and, when it fails, a concrete fix. `--fix` does only what `check` does
//! safely: it removes locks and temp files whose writer is gone.

use crate::fsx;
use crate::json::Value;
use crate::store::Store;
use crate::tool::Args;

struct Check {
    name: &'static str,
    ok: bool,
    detail: String,
    fix: Option<String>,
}

fn pass(name: &'static str, detail: String) -> Check {
    Check {
        name,
        ok: true,
        detail,
        fix: None,
    }
}

fn fail(name: &'static str, detail: String, fix: String) -> Check {
    Check {
        name,
        ok: false,
        detail,
        fix: Some(fix),
    }
}

fn env(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub fn run(a: &Args) -> Result<Value, String> {
    let fix = a.bool("fix").unwrap_or(false);
    let mut checks = vec![pass(
        "version",
        format!(
            "libellus {} on {} {}",
            crate::VERSION,
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    )];
    let agent = env("LIBELLUS_AGENT");
    checks.push(match &agent {
        Some(name) if !crate::board::valid_agent(name) => fail(
            "agent",
            format!(
                "LIBELLUS_AGENT is \"{}\", which is not an agent name",
                fsx::clip(name, 40)
            ),
            "set LIBELLUS_AGENT to lowercase letters, digits and '-' (main, sub-2), or unset it"
                .into(),
        ),
        Some(name) => pass("agent", format!("LIBELLUS_AGENT is {name}")),
        None => pass(
            "agent",
            "LIBELLUS_AGENT is not set: every call passes agent".into(),
        ),
    });
    let store = Store::open();
    let dir_env = env("LIBELLUS_DIR");
    let store = match store {
        Ok(s) => {
            checks.push(pass(
                "store",
                format!(
                    "{}{}",
                    s.dir.display(),
                    match &dir_env {
                        Some(_) => " (from LIBELLUS_DIR)",
                        None => " (found walking up from the working folder)",
                    }
                ),
            ));
            Some(s)
        }
        Err(e) => {
            checks.push(fail(
                "store",
                e,
                match &dir_env {
                    Some(d) => format!("run libellus init with LIBELLUS_DIR={d}, or fix LIBELLUS_DIR"),
                    None => "run libellus init in the project folder, or set LIBELLUS_DIR to the .libellus folder".into(),
                },
            ));
            None
        }
    };
    if let Some(store) = &store {
        checks.push(writable(store));
        let mut removed = Vec::new();
        if fix {
            removed = crate::reap::reap_abandoned(&store.dir);
        }
        let leftovers = crate::reap::find_abandoned(&store.dir);
        checks.push(if leftovers.is_empty() {
            pass(
                "stray_files",
                if removed.is_empty() {
                    "no abandoned temp files or 1.3 lock files".into()
                } else {
                    format!("removed {}", removed.join(", "))
                },
            )
        } else {
            fail(
                "stray_files",
                format!(
                    "temp files over an hour old or lock files from Libellus 1.3: {}",
                    clip_list(&leftovers)
                ),
                "run libellus doctor --fix (or libellus check), which removes only those files; the lock files in locks/ stay".into(),
            )
        });
        checks.push(file_system(store));
        checks.push(match crate::check::scan(store) {
            Ok(scan) => {
                let problems = scan.problems;
                if problems.is_empty() {
                    pass(
                        "store_files",
                        format!(
                            "{} agents, {} board tasks, {} archived projects read without damage",
                            scan.agents.len(),
                            scan.board_tasks,
                            scan.archived_projects
                        ),
                    )
                } else {
                    fail(
                        "store_files",
                        clip_list(&problems),
                        "each problem names its file. Fix or remove that file by hand; Libellus never resets a damaged file. Then run libellus check".into(),
                    )
                }
            }
            Err(e) => fail(
                "store_files",
                e,
                "fix the named file or folder, then run libellus check".into(),
            ),
        });
    }
    checks.push(groups_check());
    checks.push(mcp_binary());
    checks.push(match crate::promote::find_relatum() {
        Ok(dir) => pass("relatum", format!("libellus_promote writes to {}", dir.display())),
        Err(e) if e.starts_with("no .swarmnotes") => Check {
            name: "relatum",
            ok: true,
            detail: "no Relatum store; libellus_promote is unavailable until there is one".into(),
            fix: Some("run relatum init in the project folder, or set SWARMNOTES_DIR, if you want to promote notes".into()),
        },
        Err(e) => fail(
            "relatum",
            e,
            "remove or replace the named .swarmnotes, or set SWARMNOTES_DIR to the store".into(),
        ),
    });
    if cfg!(windows) {
        if let Some(store) = &store {
            checks.push(path_length(&store.dir.display().to_string()));
        }
    }
    let ok = checks.iter().all(|c| c.ok);
    Ok(Value::obj(vec![
        ("ok", Value::Bool(ok)),
        (
            "checks",
            Value::Array(
                checks
                    .into_iter()
                    .map(|c| {
                        Value::obj(vec![
                            ("name", Value::str(c.name)),
                            ("ok", Value::Bool(c.ok)),
                            ("detail", Value::String(c.detail)),
                            ("fix", c.fix.map(Value::String).unwrap_or(Value::Null)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ]))
}

fn clip_list(items: &[String]) -> String {
    let mut shown: Vec<String> = items.iter().take(10).map(|s| fsx::clip(s, 300)).collect();
    if items.len() > 10 {
        shown.push(format!("and {} more", items.len() - 10));
    }
    shown.join("; ")
}

/// Create and remove a probe file in the store with the same helpers every
/// write uses. Its name is a temp name, so a kill mid-probe is cleaned up
/// like any other temp file.
fn writable(store: &Store) -> Check {
    let name = format!(".doctor.tmp-{}-probe", std::process::id());
    let probe = match fsx::join_child(&store.dir, &[&name]) {
        Ok(p) => p,
        Err(e) => return fail("writable", e, "check the store folder".into()),
    };
    match fsx::write_atomic(&probe, b"probe\n", false) {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            pass(
                "writable",
                format!("created and removed a file in {}", store.dir.display()),
            )
        }
        Err(e) => fail(
            "writable",
            e,
            format!(
                "give this user write access to {} (or point LIBELLUS_DIR at a writable store)",
                store.dir.display()
            ),
        ),
    }
}

/// A store on a network file system: OS file locks there may be emulated,
/// local to one machine, or ignored. A warning, not a failure.
fn file_system(store: &Store) -> Check {
    match crate::oslock::network_fs(&store.dir) {
        Some(kind) => Check {
            name: "file_system",
            ok: true,
            detail: format!(
                "the store is on a network file system ({kind}); file locks there may be emulated or not shared between machines, so two writers could both get a lock"
            ),
            fix: Some("keep the store on a local disk, or let only one machine write to it".into()),
        },
        None => pass(
            "file_system",
            "local, or not detectable on this platform: file locks are reliable".into(),
        ),
    }
}

fn groups_check() -> Check {
    let project = crate::store::project_dir();
    let enabled = crate::groups::enabled(
        crate::tools::TOOLS,
        "libellus",
        crate::tools::GROUPS,
        project.as_deref(),
    );
    let mut on: Vec<&str> = Vec::new();
    for t in &enabled.tools {
        if !on.contains(&t.group) {
            on.push(t.group);
        }
    }
    let source = if env("LIBELLUS_TOOLS").is_some() || env("LIBELLUS_DISABLE").is_some() {
        "LIBELLUS_TOOLS / LIBELLUS_DISABLE".to_string()
    } else {
        match &project {
            Some(p) if p.join("AGENTS.md").is_file() => {
                format!("{}", p.join("AGENTS.md").display())
            }
            _ => "no AGENTS.md (every group on)".to_string(),
        }
    };
    match enabled.notice {
        Some(n) => fail(
            "tool_groups",
            n,
            format!(
                "write one line such as \"libellus: disable ledger\" in the agent-tools block (groups: {}); core cannot be disabled",
                crate::tools::GROUPS.join(", ")
            ),
        ),
        None => pass("tool_groups", format!("on: {} (from {source})", on.join(", "))),
    }
}

fn mcp_binary() -> Check {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            return fail(
                "mcp_binary",
                format!("cannot find this program: {e}"),
                "run libellus from its install folder".into(),
            )
        }
    };
    let name = format!("libellus-mcp{}", std::env::consts::EXE_SUFFIX);
    let beside = exe.with_file_name(&name);
    if beside.is_file() {
        pass(
            "mcp_binary",
            format!("{} is next to this binary", beside.display()),
        )
    } else {
        fail(
            "mcp_binary",
            format!("no {name} in {}", exe.parent().map(|p| p.display().to_string()).unwrap_or_default()),
            format!("copy {name} from the release archive next to libellus, or set the MCP command to \"libellus\" with args [\"mcp\"]"),
        )
    }
}

/// Windows refuses most paths over 260 characters unless long paths are on.
/// A note path is the store folder plus `\agents\<agent>\notes\<id>.md` and
/// a temp suffix while it is written: about 100 more with ids made from a
/// summary (48 characters). Ids of the full 64 characters need more.
fn path_length(store: &str) -> Check {
    let worst = store.chars().count() + 100;
    if worst > 260 {
        fail(
            "path_length",
            format!(
                "the store path is {} characters; the longest note path could reach {worst}, past the Windows limit of 260",
                store.chars().count()
            ),
            "move the project to a shorter folder (C:\\src\\proj), or turn on long paths (LongPathsEnabled) and keep ids short".into(),
        )
    } else {
        pass(
            "path_length",
            format!("longest path about {worst} characters"),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_long_store_path_fails_with_a_fix() {
        assert!(super::path_length("C:\\p\\.libellus").ok);
        let long = format!("C:\\{}\\.libellus", "x".repeat(170));
        let c = super::path_length(&long);
        assert!(!c.ok && c.fix.unwrap().contains("shorter"));
    }
}
