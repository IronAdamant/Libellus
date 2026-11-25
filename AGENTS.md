# Libellus

A private notepad and number ledger for each AI agent. One std-only Rust crate:
the `libellus` CLI and the `libellus-mcp` stdio server (`libellus mcp` is the
same program). Read `SPEC.md` for the format and `README.md` for what is built.

## Rules

- `[dependencies]` and `[dev-dependencies]` stay empty; `tests/zero_deps.rs`
  checks it, and `Cargo.lock` holds one package. Do not add a crate.
- Never call a model. The agent reports numbers; Libellus adds them.
- No TODO or FIXME markers in `src/`.
- Docs use Australian spelling and plain ASCII: no em-dashes.
- A damaged store is an error. Never read it as empty, never reset it.
- Ids, agent names and evidence paths are validated. Do not build a path from
  user input without `fsx::join_id` or `fsx::project_file`.
- Writes go through `fsx::write_atomic`. New notes use `overwrite = false`.
- Unknown arguments are errors. New tools go in the `tools::TOOLS` table with
  their parameters, so the schema, the checks and the CLI flags stay in step.
- Notes stay private per agent. Sharing is the promote step into Relatum, not a
  shared scope here.
- Out of scope, because other tools own it: code memory (Stele-Context), per-file
  change notes (Wikifier), plans and constraints (Trammel), live coordination and
  locks (CoordinationHub), swarm-shared notes (Relatum), cross-tool verification
  (Adsertio).

## Commands

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

## Layout

- `src/json.rs` bounded JSON. `src/fsx.rs` atomic writes, path and slug guards.
- `src/tool.rs` tool table types and argument validation. `src/tools.rs` the tools.
- `src/mcp.rs` the server. `src/groups.rs` the `agent-tools` block.
- `src/note.rs` notes. `src/ledger.rs` the ledger. `src/store.rs` store lookup.
- `src/cli.rs` the command line.
- `tests/hostile_input.rs`, `tests/store_integrity.rs`, `tests/cli_and_groups.rs`:
  the failure modes the design avoids. Keep them passing.
