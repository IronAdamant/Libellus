# Libellus

A private notepad and number ledger for each AI agent, plus one shared task
board. One std-only Rust crate:
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
  `.swarmnotes/` yourself. The board (`.libellus/board/`) is the one
  deliberate shared part: the agents' own working task list. Board writes
  take the one board lock (`locks/board.lock`) from read to last write.
- Anything that writes takes a batch, so an agent spends one call, not one
  per item.
- Out of scope, because other tools own it: code memory (Stele-Context), per-file
  change notes (Wikifier), plans of code changes in dependency order and their
  verification (Trammel; the Libellus board is the agents' own working task
  list, not that), live coordination and locks (CoordinationHub), swarm-shared
  notes (Relatum), cross-tool verification (Adsertio).

## Commands

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

## Layout

- `src/json.rs` bounded JSON. `src/fsx.rs` atomic writes, path and slug guards.
  `src/reap.rs` old temp files and 1.3 lock files. `src/seq.rs` the note sequence.
  `src/oslock.rs` operating-system file locks (flock, LockFileEx) and the
  network file system probe; std only and self-contained, so it can be
  copied into another crate.
- `src/tool.rs` tool table types and argument validation. `src/tools.rs` the tools.
  `src/brief.rs` the session brief. `src/ledger_meta.rs` the spend totals cache.
- `src/mcp.rs` the server. `src/groups.rs` the `agent-tools` block and
  `LIBELLUS_TOOLS` / `LIBELLUS_DISABLE`.
- `src/note.rs` notes. `src/note_cmd.rs` note and resolve, single or batched.
  `src/ledger.rs` the ledger. `src/ledger_cmd.rs` spend and ledger commands.
- `src/board.rs` board tasks, loading and the tree. `src/plan.rs` `libellus_plan`:
  ops, the outline, cascades, the board lock and writes. `src/board_view.rs`
  `libellus_board` and the brief's Board section. `src/html.rs` `board.html`.
- `src/doctor.rs` `libellus doctor`.
- `src/store.rs` store lookup and `Store::lock`.
- `src/promote.rs` the Relatum promote step. `src/hook.rs` the hook snippet and
  `hook-run` (what the Claude Code hooks run).
- `src/cli.rs` the command line. `LIBELLUS_AGENT` fills a required agent.
- `src/check.rs` the store check used by `libellus check` and doctor
  (notes, ledgers and the board).
- `Store::lock(Locked::...)` takes the operating-system lock in
  `.libellus/locks/` around resolve, promote, spend, archive, ledger reads,
  the seq counters and every board write. The kernel releases it when the
  holder exits or is killed. Never delete a lock file, and do not add a
  stale-lock rule (pid, age or token): there is no stale lock.
- `examples/perf.rs` release timings. `docs/perf/` the recorded numbers.
- `tests/hostile_input.rs`, `tests/store_integrity.rs`, `tests/cli_and_groups.rs`,
  `tests/note_life.rs`, `tests/ledger_groups.rs`, `tests/promote.rs`,
  `tests/hook.rs`, `tests/agent_ux.rs`, `tests/review4.rs`, `tests/review5.rs`,
  `tests/v130.rs` (batches, the board, the page, doctor, hooks), `tests/oslock.rs`
  (locks across processes, a killed holder, many writers, 1.3 leftovers): the failure
  modes the design avoids. `tests/surface.rs` holds `tools/list` to 8,500
  bytes and the instructions under 600: keep tool and parameter descriptions
  short; batch item fields carry no description.
  Keep them passing.
