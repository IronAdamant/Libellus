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
    instructions: "Libellus is your private notepad and number ledger. Write decisions, dead ends and open questions as you work (libellus_note), point at evidence as tool:reference instead of copying it, and call libellus_brief after your context is compacted to get your open questions, recent decisions and running costs back. Notes are private to your agent name; share with the swarm by writing a Relatum note.",
    tools: tools::TOOLS,
    groups: tools::GROUPS,
    project: store::project_dir,
};
