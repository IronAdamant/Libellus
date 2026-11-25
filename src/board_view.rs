//! Reading the board: `libellus_board`, and the brief's Board section.

use crate::board::{self, is_open, Board, Filter};
use crate::json::Value;
use crate::store::Store;
use crate::tool::Args;

/// Most bytes of tree text `libellus_board` returns.
const MAX_TEXT: usize = 16_384;

pub fn run(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let agent = a.str("agent").unwrap_or_default();
    board::check_agent(agent, "agent")?;
    if let Some(o) = a.str("owner") {
        board::check_agent(o, "owner")?;
    }
    let live = board::load(&store)?;
    let filter = Filter {
        owner: a.str("owner"),
        project: a.str("project"),
        open_only: a.bool("open_only").unwrap_or(true),
    };
    if let Some(p) = filter.project {
        let archived_match = a.bool("include_archived").unwrap_or(false);
        if live.get(p).is_none_or(|t| !t.parent.is_empty()) && !archived_match {
            return Err(format!(
                "no open project \"{}\" on the board. Projects: {}. Pass include_archived to search the archive.",
                crate::fsx::clip(p, 60),
                live.roots().join(", ")
            ));
        }
    }
    let keep = board::shown(&live, &filter);
    let mut text = board::render_tree(&live, Some(&keep), agent);
    let mut archived_count = 0u64;
    if a.bool("include_archived").unwrap_or(false) {
        for arch in board::load_archive(&store)? {
            let root = arch.board.roots().first().cloned().unwrap_or_default();
            if filter.project.is_some_and(|p| p != root && p != arch.name) {
                continue;
            }
            let keep = board::shown(
                &arch.board,
                &Filter {
                    owner: filter.owner,
                    project: None,
                    open_only: false,
                },
            );
            if keep.is_empty() {
                continue;
            }
            archived_count += 1;
            text.push_str(&format!("archived {}:\n", arch.name));
            text.push_str(&board::render_tree(&arch.board, Some(&keep), agent));
        }
    }
    let mut truncated = false;
    if text.len() > MAX_TEXT {
        let mut end = MAX_TEXT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let cut = text[..end].rfind('\n').map(|i| i + 1).unwrap_or(end);
        text.truncate(cut);
        text.push_str("Cut at 16 KiB. Narrow with project or owner.\n");
        truncated = true;
    }
    if text.is_empty() {
        text = if live.tasks.is_empty() {
            "The board is empty. Plan with libellus_plan (an outline makes a whole tree in one call).\n".to_string()
        } else {
            "No task matches.\n".to_string()
        };
    }
    let mut pairs = vec![
        ("text", Value::String(text)),
        ("counts", live.counts().to_value()),
        ("shown", Value::uint(keep.len() as u64)),
    ];
    if archived_count > 0 {
        pairs.push(("archived_shown", Value::uint(archived_count)));
    }
    if truncated {
        pairs.push(("truncated", Value::Bool(true)));
    }
    if !live.warnings.is_empty() {
        pairs.push((
            "warnings",
            Value::Array(live.warnings.iter().map(Value::str).collect()),
        ));
    }
    Ok(Value::obj(pairs))
}

/// The brief's view of the board for one agent.
pub struct Section {
    pub text: String,
    pub value: Value,
    pub open_mine: usize,
    pub truncated: bool,
}

/// Open tasks owned by `agent` that have no open children, doing first, then
/// blocked, then todo, each with its path from the project, capped at `max`. One counts line for the
/// whole board follows.
pub fn section(live: &Board, archived: usize, agent: &str, max: usize) -> Section {
    let rank = |s: &str| match s {
        "doing" => 0,
        "blocked" => 1,
        _ => 2,
    };
    let mut mine: Vec<(usize, usize, &board::Task)> = live
        .tree_order()
        .into_iter()
        .enumerate()
        .filter(|(_, (t, _))| t.owner == agent && is_open(&t.status))
        // A parent with open children closes by itself; its children are the
        // work, and each line's path names the parents.
        .filter(|(_, (t, _))| {
            !live
                .children(&t.id)
                .iter()
                .any(|c| live.get(c).is_some_and(|c| is_open(&c.status)))
        })
        .map(|(i, (t, _))| (rank(&t.status), i, t))
        .collect();
    mine.sort_by_key(|(r, i, _)| (*r, *i));
    let total = mine.len();
    let mut text = String::from("## Board\n");
    let mut rows = Vec::new();
    for (_, _, t) in mine.iter().take(max) {
        let path = live.path(&t.id).join(" > ");
        text.push_str(&format!(
            "- {} {}: {}\n",
            board::mark(&t.status),
            path,
            t.title
        ));
        rows.push(Value::obj(vec![
            ("id", Value::str(&t.id)),
            ("title", Value::str(&t.title)),
            ("status", Value::str(&t.status)),
            ("path", Value::String(path)),
        ]));
    }
    if total == 0 {
        if live.tasks.is_empty() {
            text.push_str("- none. Plan shared work with libellus_plan.\n");
        } else {
            text.push_str(&format!("- none open for {agent}. See libellus_board.\n"));
        }
    } else if total > max {
        text.push_str(&format!(
            "Showing {} of your {total} open tasks. The rest are in libellus_board.\n",
            max.min(total)
        ));
    }
    let c = live.counts();
    text.push_str(&format!(
        "- board: {}, {} archived projects\n",
        c.line(),
        archived
    ));
    let value = Value::obj(vec![
        ("tasks", Value::Array(rows)),
        ("open_mine", Value::uint(total as u64)),
        ("counts", c.to_value()),
        ("archived_projects", Value::uint(archived as u64)),
    ]);
    Section {
        text,
        value,
        open_mine: total,
        truncated: total > max,
    }
}

/// How many archived projects there are, without parsing them.
pub fn archived_count(store: &Store) -> usize {
    board::archive_dir(store)
        .ok()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.file_type().is_ok_and(|t| t.is_dir())
                        && !e.file_name().to_string_lossy().starts_with('.')
                })
                .count()
        })
        .unwrap_or(0)
}
