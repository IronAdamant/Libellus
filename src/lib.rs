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
    instructions: "Libellus is your private notepad and number ledger. Write decisions, dead ends and open questions as you work (libellus_note), read one back with libellus_get, and close a question with libellus_resolve. Batch: libellus_note takes notes and libellus_resolve takes items, so several cost one call. The board (libellus_plan, libellus_board) is the one thing shared by every agent here: the main agent plans a project as a task tree and hands subtrees to sub-agents (owner), and each agent ticks off its own tasks in batches; parents close by themselves. Point at evidence as tool:reference instead of copying it. Call libellus_brief when a session starts and after your context is compacted: it lists your open board tasks too. Record tokens and cost with libellus_spend (you report the numbers; Libellus only adds them) and read totals with libellus_ledger. Notes are private to your agent name. Share one with the swarm only by libellus_promote, which writes a Relatum note. Libellus does not call a model.",
    tools: tools::TOOLS,
    groups: tools::GROUPS,
    project: store::project_dir,
};
