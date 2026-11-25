# Libellus

A private notepad and number ledger for each AI agent. One std-only Rust crate:
the `libellus` CLI and the `libellus-mcp` stdio server (`libellus mcp` is the
same program). Read `SPEC.md` for the format and `README.md` for what is built.

## Rules

- `[dependencies]`, `[dev-dependencies]` and `[build-dependencies]` stay empty;
  `tests/zero_deps.rs` checks it, and `Cargo.lock` holds one package. Do not
  add a crate.
- Never call a model. The agent reports numbers; Libellus adds them.
- No TODO or FIXME markers in `src/`.
- Docs use Australian spelling and plain ASCII: no em-dashes.
- A damaged store is an error. Never read it as empty, never reset it.
- Ids, agent names and evidence paths are validated. Do not build a path from
  user input without `fsx::join_id`, `fsx::join_child` or `fsx::project_file`.
- Writes go through `fsx::write_atomic`. New notes, and a new Relatum note on
  promote, use `overwrite = false`. Resolve and the `promoted` update use
  `overwrite = true` only on that note's own file.
- Unknown arguments are errors. New tools go in the `tools::TOOLS` table with
  their parameters, so the schema, the checks and the CLI flags stay in step.
  `libellus hook` is a CLI command, not an MCP tool.
- Notes stay private per agent. Sharing is `libellus_promote` into Relatum,
  not a shared scope here. Do not run the `relatum` binary and do not create
  `.swarmnotes/` yourself.
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
- `src/mcp.rs` the server. `src/groups.rs` the `agent-tools` block and
  `LIBELLUS_TOOLS` / `LIBELLUS_DISABLE`.
- `src/note.rs` notes. `src/ledger.rs` the ledger. `src/store.rs` store lookup.
- `src/promote.rs` the Relatum promote step. `src/hook.rs` the hook snippet.
- `src/cli.rs` the command line. `LIBELLUS_AGENT` fills a required agent.
- `examples/perf.rs` release timings. `docs/perf/` the recorded numbers.
- `tests/hostile_input.rs`, `tests/store_integrity.rs`, `tests/cli_and_groups.rs`,
  `tests/note_life.rs`, `tests/ledger_groups.rs`, `tests/promote.rs`,
  `tests/hook.rs`, `tests/agent_ux.rs`: the failure modes the design avoids.
  Keep them passing.
