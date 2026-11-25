# Libellus

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**A private notepad and number ledger for each AI agent.**

An agent loses its working memory when its context is compacted: the question it
was still unsure about, the approach it already ruled out, the decision it made
an hour ago, what the run has cost so far. Libellus keeps those in plain files
the agent owns, and hands back a short brief when the context is squashed.

Libellus is one small Rust binary (standard library only, no Cargo crates) with a
CLI and a stdio MCP server over the same files. It never calls a model. No
service, no account.

> **Status: v0.1.0, a scaffold.** The foundations and four tools work and are
> tested. The rest of the design is written down in [SPEC.md](SPEC.md) and not
> built yet. See "What works today".

## What it holds

```
.libellus/
  agents/main/notes/retry/post-question.md   <- a note, private to the agent "main"
  agents/main/ledger.jsonl                   <- tokens and cost, one line per step
  agents/sub-2/...                           <- each subagent has its own folder
```

- **Notes** are short markdown files with a little frontmatter, in the same
  family as [Relatum](https://github.com/IronAdamant/Relatum) notes. A note has a
  `kind` (decision, dead-end, question, finding, context) and a `status` (open or
  resolved). Questions start open.
- **Evidence is a pointer, not a copy.** A note names where proof lives, as
  `tool:reference` (`trammel:plan-3/step-2`, `chisel:tests/net.rs`,
  `probatio:runs/7`). Nothing is pasted in.
- **The ledger** records the tokens and cost an agent reports for each task and
  step. Cost is a whole number of millionths of a US dollar, so sums are exact.
  Libellus adds numbers up; it does not know any model's price.
- **The brief** (`libellus_brief`) is what comes back after compaction: open
  questions, recent decisions, dead ends, and running costs.

## What it is not

| Tool | What it holds | Libellus instead |
|---|---|---|
| Relatum | Notes shared across a swarm, monitored by the parent | Private to one agent. Sharing is a deliberate promote into Relatum. |
| Stele-Context | Code memory | Holds no code, only what the agent thinks about it. |
| Wikifier | Per-file notes on why a file changed | One agent's working notes, not tied to a file. |
| Trammel | Plans and constraints | Notes can point at a plan step; they do not hold the plan. |
| CoordinationHub | Live coordination and locks | No locks, no live state. |

"Private" means separate per agent and not shared by default. It is not secret:
anything running as the same operating system user can read the files.

## What works today

MCP tools (and the same as CLI commands without the `libellus_` prefix):

| Tool | Group | Does |
|---|---|---|
| `libellus_note` | core | Write a note. Never overwrites an existing one. |
| `libellus_ls` | core | List your notes with filters, optionally with bodies. |
| `libellus_brief` | core | The compaction payload. |
| `libellus_spend` | ledger | Record tokens and cost for a step. |

```sh
cargo build --release
cd your-project
libellus init
libellus note --agent main --kind question --summary "Does retry apply to POST?" --evidence trammel:plan-3/step-2
libellus spend --agent main --task retry --tokens_in 1200 --tokens_out 300 --cost_micro_usd 15000
libellus brief --agent main
```

Not built yet: resolving a note, reading one note by id, the promote step into
Relatum, the compaction hook wiring, ledger totals by step or agent, release
binaries and npm packages. SPEC.md lists each with its intended shape.

## Built to fail loudly

These are the failure modes found across similar tools, designed out from the
start and covered by tests in `tests/`:

- Deeply nested JSON, invalid UTF-8, oversized lines and batches get a JSON-RPC
  error. The server keeps running.
- Unknown methods are errors. A failed tool call sets `isError`.
- Unknown or misspelled arguments are errors, never ignored.
- A damaged note or ledger line is an error naming the file. It is never read as
  empty and never reset, and a damaged ledger is never appended to.
- Ids and agent names are slugs. Paths that leave the store, and symlinks, are
  refused.
- Every write lands whole through a temp file that is cleaned up on failure.
  A new note is created with a hard link, so it cannot overwrite another.
- A mistake in the `agent-tools` line falls back to the core tools and says why.

## Choosing tool groups

Add a line to the `agent-tools` block in the project's AGENTS.md. The server
rereads it on every request.

````markdown
```agent-tools
libellus: disable ledger
```
````

`libellus: tools core, ledger` lists only what you want instead. See
[AGENTS.snippet.md](AGENTS.snippet.md) for text to paste into a project.

`LIBELLUS_TOOLS` and `LIBELLUS_DISABLE`, when set, replace that line. `core`
cannot be disabled. A typo falls back to the core tools and says why.

## Compaction hook

After a session starts, and again after the context is compacted, the brief
should be put back into the session. The agent often forgets to ask.
`libellus hook` prints a snippet a person can paste into the host's hook file.
It does not install the hook, and it does not run in the background.

```sh
libellus hook --agent main --host generic
libellus hook --agent main --host claude
libellus hook --agent main --host cursor
```

Run it at session start and after compaction. The generic snippet is the
command `libellus brief --agent main`. The claude snippet is a SessionStart
hook, including the compact matcher, which runs after compaction. That
command's plain text is added to the session. The cursor snippet is a
sessionStart hook: the command prints `{"additional_context": "..."}` and
Cursor adds that text when a chat starts. Cursor's preCompact hook cannot put
text back into the model, so after compaction in Cursor you run the brief
again and put the text into the session yourself.

`LIBELLUS_AGENT` fills in `--agent` when a command needs an agent and the flag
is left out. `libellus ledger` does not use it. With no `--agent`, the ledger
sums every agent in the store.

## Build and test

```sh
cargo test --locked
cargo build --release --locked
```

Rust 1.83. `Cargo.toml` has no dependencies; `tests/zero_deps.rs` checks it.

## Licence

MIT. See [LICENSE](LICENSE).
