//! Libellus: a private notepad and number ledger for each AI agent.
//! Standard library only. See basis.md for the design and SPEC.md for the format.

pub mod cli;
pub mod fsx;
pub mod groups;
pub mod hook;
pub mod json;
pub mod ledger;
pub mod mcp;
pub mod note;
pub mod promote;
pub mod store;
pub mod tool;
pub mod tools;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub static SERVER: mcp::Server = mcp::Server {
    name: "libellus",
    version: VERSION,
    instructions: "Libellus is your private notepad and number ledger. Write decisions, dead ends and open questions as you work (libellus_note), read one back with libellus_get, and close a question with libellus_resolve. Point at evidence as tool:reference instead of copying it. Call libellus_brief when a session starts and after your context is compacted. Record tokens and cost with libellus_spend (you report the numbers; Libellus only adds them) and read totals with libellus_ledger. Notes are private to your agent name. Share one with the swarm only by libellus_promote, which writes a Relatum note. Libellus does not call a model.",
    tools: tools::TOOLS,
    groups: tools::GROUPS,
    project: store::project_dir,
};
