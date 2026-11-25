# Changelog

## 0.1.0 (unreleased)

The scaffold.

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
