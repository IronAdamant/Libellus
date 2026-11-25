# Libellus

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Libellus is a private notepad and cost ledger for an AI agent, so it does not
lose track of its work when its conversation is compacted.

When a long session gets compacted (the earlier conversation squashed into a
summary), an agent tends to forget the question it was still unsure about,
the approach it already ruled out, the decision it made, and how much the run
has cost so far. Libellus keeps those in plain files that the agent owns.
After compaction, one command gives back a short brief to paste into the
session.

Each agent gets its own folder of short notes, plus a ledger of the tokens and
cost it reports. Notes can point at proof elsewhere (`tool:reference`) rather
than copying it. Sharing a note with other agents is a deliberate step into
[Relatum](https://github.com/IronAdamant/Relatum), not the default.

What it deliberately does not do:

- **Call a model.** The agent reports its tokens and cost; Libellus only adds
  them up. Cost is a whole number of millionths of a US dollar (micro-USD).
- **Keep secrets.** "Private" means separate per agent. Anyone with access to
  the same computer account can read the files.
- **Do other tools' jobs.** Shared swarm notes are
  [Relatum](https://github.com/IronAdamant/Relatum), code memory is
  Stele-Context, per-file change notes are Wikifier, plans are
  [Trammel](https://github.com/IronAdamant/Trammel), live locks are
  CoordinationHub, and checking claims is
  [Adsertio](https://github.com/IronAdamant/Adsertio).

Libellus is one small Rust program with no dependencies beyond the standard
library. It works as a command line tool (`libellus`) and as an MCP server for
AI agents (`libellus-mcp`, or `libellus mcp`). There is no service and no
account.

## Install

Download the archive for your platform from the
[latest release](https://github.com/IronAdamant/Libellus/releases/latest):

| Platform | Archive |
|---|---|
| Linux x64 | `libellus-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `libellus-<version>-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `libellus-<version>-aarch64-apple-darwin.tar.gz` |
| Windows x64 | `libellus-<version>-x86_64-pc-windows-msvc.zip` |

Each archive holds `libellus` and `libellus-mcp`. Check the download against
`SHA256SUMS` before you run it. Libellus is not on crates.io or npm.

Or build it yourself (Rust 1.83 or newer):

```sh
git clone https://github.com/IronAdamant/Libellus
cd Libellus
cargo build --release --locked
```

The binaries land in `target/release/`.

## A quick tour

```sh
cd your-project
libellus init                                  # creates .libellus/ here

# Note a question, then answer it later.
libellus note --agent main --kind question --summary "Does retry apply to POST?"
libellus resolve --agent main --id does-retry-apply-to-post --outcome "No, GET only."
libellus get --agent main --id does-retry-apply-to-post

# Record what a step cost, and see the totals.
libellus spend --agent main --task retry --tokens_in 100 --tokens_out 20 --cost_micro_usd 1000
libellus ledger --agent main

# The text to put back into the session after compaction.
libellus brief --agent main
```

Set `LIBELLUS_AGENT=main` and you can leave out `--agent`. (`libellus ledger`
is the exception: with no `--agent` it totals every agent.) Flags can be
written `--name value` or `--name=value`. A value that itself starts with `--`
is written `--name -- --value`.

The brief is plain text, short enough to paste straight back in:

```text
# Libellus brief for main

## Current state
- retry-wrapper-...: Retry wrapper is in src/http.rs; GET is done, POST next

## Open questions (1)
- does-retry-apply-to-put: Does retry apply to PUT?

## Recent decisions
- retry-only-idempotent-methods: Retry only idempotent methods

## Dead ends
- global-retry-middleware-...: Global retry middleware: breaks streaming uploads

## Costs
- total: 12000 in, 3000 out tokens, 45000 micro-USD (1 ledger entries)
- retry: 12000 in, 3000 out, 45000 micro-USD

Next: resolve an open question with libellus_resolve; share a note with libellus_promote.
```

Each section is capped (5 items by default, `--max_items` up to 50), newest
first.

### Notes

Every note has a **kind**:

| Kind | Use it for |
|---|---|
| `context` | where things stand right now (the newest one is the brief's "Current state") |
| `decision` | a choice you made, and why |
| `question` | something still unsure; starts open until you resolve it |
| `dead-end` | an approach you tried and ruled out |
| `finding` | something you learned |

A note is `open` or `resolved`. Questions start open; other kinds start
resolved. To mark a decision as replaced, resolve it with `--supersede`, or
with an outcome that starts `SUPERSEDED:` or `REVERSED:`. Evidence is a
pointer such as `trammel:plan-3/step-2`, not a copy of the proof.

### Where the files live

The store is the nearest `.libellus/` folder, walking up from the current
directory, or the folder named by `$LIBELLUS_DIR`.

```
.libellus/
  agents/main/notes/does-retry-apply-to-post.md
  agents/main/seq                 the note counter
  agents/main/ledger.jsonl        one line per spend
  agents/main/ledger.meta         totals cache, rebuilt from the ledger
  agents/main/archive/            old ledgers moved aside by archive
  agents/sub-2/...
```

The ledger only grows. When it gets large, `libellus archive --agent main`
moves it into `archive/` and starts a fresh one; the old file is kept but no
longer counted. The file format is in [SPEC.md](SPEC.md).

## Using it from an agent (MCP)

For Claude Code (`.mcp.json`) or Cursor (`.cursor/mcp.json`):

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
project.

| Tool | Group | Does |
|---|---|---|
| `libellus_note` | core | Write a note. Never overwrites an existing one. |
| `libellus_get` | core | Read one note, including its body. |
| `libellus_resolve` | core | Mark a note resolved and add a one-line outcome. `supersede` marks it replaced. |
| `libellus_ls` | core | List your notes. Page with `before_id`. |
| `libellus_brief` | core | The compaction brief. Still returns the notes if the ledger cannot be read. |
| `libellus_check` | core | Read every note and ledger, and fail on damage. Clears locks and temp files left by a writer that is gone. |
| `libellus_promote` | core | Copy a note into Relatum and record its Relatum id. |
| `libellus_spend` | ledger | Record tokens and cost for a step. Refuses once the ledger would pass 32 MiB. |
| `libellus_ledger` | ledger | Totals, by task, by step, or across every agent. |
| `libellus_archive` | ledger | Move the ledger into `archive/` and start a fresh one. |

Each tool is also a CLI command without the `libellus_` prefix.

### Run the brief automatically

`libellus hook --agent NAME --host claude|cursor|generic` prints a snippet to
paste into the host's hook settings, so the brief runs at the start of a
session and after compaction:

| Host | What the snippet does |
|---|---|
| `claude` | A Claude Code `SessionStart` hook for `.claude/settings.json`. Its matcher includes `compact`, so it also runs after compaction, and the brief is added to the session. |
| `cursor` | A Cursor `sessionStart` hook for `.cursor/hooks.json`. Cursor has no hook that can put text back after compaction, so after compacting, run the brief again (or call `libellus_brief`) and paste it in. |
| `generic` | Just the command, `libellus brief --agent NAME`, for any other host. |

### Tool groups

The `core` tools are always on. The `ledger` tools (`libellus_spend`,
`libellus_ledger`, `libellus_archive`) can be switched off with one line in
the `agent-tools` block of the project's AGENTS.md:

````markdown
```agent-tools
libellus: disable ledger
```
````

Or list what you want: `libellus: tools core, ledger`. The environment
variables `LIBELLUS_TOOLS` and `LIBELLUS_DISABLE` override that line. The
server rereads the choice on every request. A typo, or trying to disable
`core`, falls back to the core tools and says why. See
[AGENTS.snippet.md](AGENTS.snippet.md) for text to paste into a project.

## Built to fail loudly

When something is wrong, Libellus says so instead of guessing. Each of these
is covered by tests in `tests/`:

- **Damage is an error.** A damaged note or ledger is reported with the file
  name. It is never read as empty, never reset, and a damaged ledger is never
  appended to.
- **No writing outside the store.** Ids and agent names must be simple
  lowercase names. Paths that leave the store, and symlinks (including a
  symlinked `.libellus`, `notes/` or `archive/`), are refused. Promote applies
  the same checks to the Relatum path.
- **Safe writes.** Every write lands whole through a temp file. A new note
  never overwrites another, and resolve and promote replace only that note's
  own file.
- **Safe with several writers.** Resolve, promote, spend, archive and the note
  counter each take a lock. If a writer is killed, exactly one later writer
  takes over its lock, so two notes never share a number and a resolve is
  never lost.
- **No repeats.** A second resolve is an error unless it marks the note
  replaced. A second promote is an error and writes no second Relatum note.
- **A strict server.** Bad JSON, oversized messages and batches get a JSON-RPC
  error and the server keeps running. Unknown methods and misspelt arguments
  are errors. A tool that panics is caught and reported.

## Performance

1.2.0 caches the ledger totals and groups tasks in one pass. On one 4-CPU
Linux machine, release build:

| Case | 1.1.2 | 1.2.0 |
|---|---:|---:|
| `spend`, 100,000 ledger entries, one task | 274 ms | 3.9 ms |
| `brief`, 100,000 different tasks | 13.1 s | 227 ms |
| `ledger`, 100,000 different tasks | 13.5 s | 336 ms |
| Brief text, 100 open questions, `--max_items 3` | 19,001 bytes | 927 bytes |

The first `spend` after the cache is lost still reads the whole ledger (183 ms
at 100,000 entries). The full tables and limits are in
[docs/perf/1.2.0.md](docs/perf/1.2.0.md). `examples/perf.rs` reruns them.

## Build and test

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

`Cargo.toml` has no dependencies; `tests/zero_deps.rs` checks that, and
`Cargo.lock` holds one package. Pushing a tag named `v*` builds the release
archives and `SHA256SUMS`.

## Licence

MIT. See [LICENSE](LICENSE).
