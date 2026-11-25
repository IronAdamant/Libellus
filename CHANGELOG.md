# Changelog

## 1.1.0 (unreleased)

Not tagged. Same on-disk format as 1.0.0.

- `libellus_ls` takes `offset` (default 0). The reply adds `offset` and
  `returned`. `total` is still the full count. `offset` 0 is the 1.0.0 page.
- The brief text ends with one `Next:` line. The headings and cost lines are
  unchanged.
- A missing note, a duplicate id, and a second resolve name the call that
  fixes it.
- Listing and the brief read many notes at once. A spend does not parse the
  ledger a second time. Numbers are in `docs/perf/`.

## 1.0.0

First release. A `v*` tag builds `libellus` and `libellus-mcp` for Linux x64,
macOS x64, macOS arm64, and Windows x64, and publishes those archives with
SHA256SUMS on the GitHub release. Not published to crates.io or npm.

- `libellus_get` reads one note, including its body. Missing, corrupt, a
  symlink, or another agent's file is an error.
- `libellus_resolve` sets a note to `resolved` and appends a one-line outcome.
  A second resolve is an error. `created` is kept. The file is replaced whole.
- `libellus_ledger` returns totals for one agent or every agent, with optional
  task and step filters and breakdowns. A damaged ledger is still an error and
  is not repaired.
- `libellus_promote` writes a Relatum note (create-only) and records its id.
  An open question is refused. A missing `.swarmnotes/` is an error and is
  not created. A second promote does not write another file.
- `LIBELLUS_TOOLS` and `LIBELLUS_DISABLE` replace the AGENTS.md `agent-tools`
  line when set. `core` cannot be disabled. A typo falls back to the core
  tools and says why.
- `LIBELLUS_AGENT` fills a required `--agent` when the flag is omitted. The
  ledger does not use it.
- `libellus hook` prints a snippet that runs `libellus brief` at session start
  and after compaction, for a generic host, Claude Code, or Cursor.

## 0.1.0

The scaffold. Not tagged.

- Strict, bounded JSON parser: nesting capped at 64, duplicate keys, control
  characters and lone surrogates rejected.
- Stdio MCP server: invalid UTF-8, malformed or deeply nested JSON, oversized
  lines and batches each get a JSON-RPC error; unknown methods are errors; a
  failed tool call sets `isError`; a panicking tool is contained.
- Tools: `libellus_note`, `libellus_ls`, `libellus_brief`, `libellus_spend`.
  Unknown arguments are errors.
- Private per-agent notes in plain files with strict frontmatter; an
  append-only ledger of whole-number tokens and cost.
- Damaged notes and ledgers fail loudly; a damaged ledger is not appended to.
- Atomic writes with temp-file cleanup; new notes are created with a hard link
  and never overwrite.
- Tool groups chosen in the AGENTS.md `agent-tools` block; a mistake falls back
  to the core tools and says why.
- CLI: `init`, `note`, `ls`, `brief`, `spend`, `check`, `mcp`.
