//! Libellus: a private notepad and number ledger for each AI agent.
//! Standard library only. See SPEC.md for the format and README.md for what is built.

pub mod board;
pub mod board_view;
pub mod brief;
pub mod check;
pub mod cli;
pub mod doctor;
pub mod fsx;
pub mod groups;
pub mod hook;
pub mod html;
pub mod json;
pub mod ledger;
pub mod ledger_cmd;
pub mod ledger_meta;
pub mod mcp;
pub mod note;
pub mod note_cmd;
pub mod plan;
pub mod promote;
pub mod reap;
pub mod seq;
pub mod store;
pub mod tool;
pub mod tools;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub static SERVER: mcp::Server = mcp::Server {
    name: "libellus",
    version: VERSION,
    instructions: "Libellus is your private notepad and token ledger, plus one task board shared by every agent. Call libellus_brief at session start and after compaction. Note decisions, dead ends and open questions as you work (libellus_note) and close them with libellus_resolve; both take batches. Plan on the board with libellus_plan (outline or ops; owner hands a subtree to a sub-agent) and tick tasks in batches. Report tokens and cost with libellus_spend. Share a note only by libellus_promote (Relatum). Evidence is tool:reference, never a copy.",
    tools: tools::TOOLS,
    groups: tools::GROUPS,
    project: store::project_dir,
};
