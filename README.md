# Libellus

## What it is

Libellus is a private notepad and a number ledger for one AI agent. The notes
and the numbers live in plain files that agent owns. After its context is
compacted, one command returns a short brief: open questions, recent
decisions, dead ends, and what the run has cost.

## Why it exists

When an agent's context is squashed, it forgets the question it was still
unsure about, the approach it already ruled out, the decision it made, and
what the run has cost. That memory has to live outside the chat, in files the
agent owns.

Each agent has a private folder of short notes and a number list (the ledger).
Notes point at proof (`tool:reference`) instead of copying it. Sharing with
the swarm is a deliberate step into [Relatum](https://github.com/IronAdamant/Relatum),
not the default.

What it deliberately does not do:

- It does not call a model. The agent reports tokens and cost. Libellus only
  adds them up. Cost is a whole number of millionths of a US dollar.
- It is not Relatum (shared swarm notes), Stele-Context (code memory),
  Wikifier (per-file change notes), Trammel (plans), CoordinationHub (locks),
  or Adsertio (verification).
- "Private" means separate per agent, not secret. The same computer user can
  read the files.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Libellus is one small Rust binary (standard library only, no Cargo crates)
with a CLI and a stdio MCP server over the same files. No service, no account.

## Install

Download a release archive for Linux x64, macOS x64, macOS arm64, or Windows
x64 from the [GitHub releases](https://github.com/IronAdamant/Libellus/releases).
Each archive contains `libellus` and `libellus-mcp`. Check `SHA256SUMS`
before you run a binary. Libellus is not published to crates.io or npm.

Or build it from this repository (Rust 1.83 or newer):

```sh
git clone https://github.com/IronAdamant/Libellus
cd Libellus
cargo build --release --locked
```

That produces `target/release/libellus` and `target/release/libellus-mcp`
(`libellus mcp` is the same server). `--version` prints the version.

## Usage

```sh
cd your-project
libellus init
libellus note --agent main --kind question --summary "Does retry apply to POST?"
libellus resolve --agent main --id does-retry-apply-to-post --outcome "No, GET only."
libellus get --agent main --id does-retry-apply-to-post
libellus spend --agent main --task retry --tokens_in 100 --tokens_out 20 --cost_micro_usd 1000
libellus ledger --agent main
libellus brief --agent main
```

The store is the nearest `.libellus/` walking up from the working directory,
or `$LIBELLUS_DIR`.

```
.libellus/
  agents/main/notes/does-retry-apply-to-post.md
  agents/main/ledger.jsonl
  agents/sub-2/...
```

A note has a `kind` (decision, dead-end, question, finding, context) and a
`status` (open or resolved). Questions start open. Evidence is a pointer such
as `trammel:plan-3/step-2`, not a copy of the proof. The ledger stores the
tokens and cost the agent reports. The brief is the text to put back into
context. The format is in [SPEC.md](SPEC.md).

`LIBELLUS_AGENT` fills in `--agent` when a command needs an agent and the flag
is left out. `libellus ledger` does not use it: with no `--agent` it sums
every agent in the store.

## MCP tools

Claude Code (`.mcp.json`) or Cursor (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "libellus": {
      "command": "libellus-mcp",
      "args": []
    }
  }
}
```

Set `LIBELLUS_DIR` only when the host does not start the server inside the
project. The same tools are CLI commands without the `libellus_` prefix.

| Tool | Group | Does |
|---|---|---|
| `libellus_note` | core | Write a note. Never overwrites an existing one. |
| `libellus_get` | core | Read one note by id, including its body. |
| `libellus_resolve` | core | Mark a note resolved and append a one-line outcome. |
| `libellus_ls` | core | List your notes. `before_id` is the stable page cursor. `limit` is at least 1. |
| `libellus_brief` | core | The compaction payload. `Next:` is added only when there is something to do. |
| `libellus_check` | core | Read every note and ledger. Fails on damage, overflow, or a mismatched agent. |
| `libellus_promote` | core | Write the note into Relatum and record that id. |
| `libellus_spend` | ledger | Record tokens and cost for a step. |
| `libellus_ledger` | ledger | Totals, optionally by task, step, or every agent. |

`libellus check` reads every note and ledger and fails on any damage.
`libellus hook --agent NAME [--host cursor|claude|generic]` prints a snippet
that runs `libellus brief`. Paste it into the host's hook file. Run it at
session start and after compaction.

The generic snippet is the command itself. The claude snippet is a
SessionStart hook, including the compact matcher, which runs after compaction.
That command's plain text is added to the session. The cursor snippet is a
sessionStart hook: the command prints `{"additional_context": "..."}` and
Cursor adds that text when a chat starts. Cursor's preCompact hook cannot put
text back into the model, so after compaction in Cursor, run the brief again
and put the text into the session yourself.

```sh
libellus hook --agent main --host generic
```

## Tool groups

`core` is always on. `ledger` (`libellus_spend`, `libellus_ledger`) can be
switched off. The server rereads the choice on every request.

Add one line to the `agent-tools` block in the project's AGENTS.md:

````markdown
```agent-tools
libellus: disable ledger
```
````

`libellus: tools core, ledger` lists what you want instead. See
[AGENTS.snippet.md](AGENTS.snippet.md) for text to paste into a project.

`LIBELLUS_TOOLS` and `LIBELLUS_DISABLE`, when set to a non-empty value,
replace that line. Names are groups, comma-separated, and case is ignored.
`core` cannot be disabled. A typo, or an attempt to disable `core`, falls
back to the core tools only and says why in `initialize` and in `tools/list`.

## Hardening

These failure modes are covered by tests in `tests/`:

- Deeply nested JSON (cap 64), invalid UTF-8, oversized lines and batches get
  a JSON-RPC error. The server keeps running.
- Unknown methods are errors. A failed tool call sets `isError`. A panicking
  tool is caught and reported as an error.
- Unknown or misspelled arguments are errors, never ignored.
- A damaged note or ledger is an error naming the file. It is never read as
  empty, never reset, and a damaged ledger is never appended to.
- Ids and agent names are slugs. Paths that leave the store, and symlinks,
  are refused. The same checks apply to a Relatum path on promote.
- Writes land whole through a temp file that is removed on failure. A new
  note is created with a hard link, so it cannot overwrite another. Resolve
  and promote update an existing note only by replacing that file.
- A second resolve is an error and does not append again. A second promote is
  an error and does not write a second Relatum note.
- A mistake in the `agent-tools` line, or in `LIBELLUS_TOOLS` /
  `LIBELLUS_DISABLE`, falls back to the core tools and says why.

## Build and test

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

`Cargo.toml` has no dependencies. `tests/zero_deps.rs` checks that, and
`Cargo.lock` holds one package.

`examples/perf.rs` times the hot paths. The 1.0.0 numbers and the later run
are in [docs/perf/](docs/perf/).

## Licence

MIT. See [LICENSE](LICENSE).
