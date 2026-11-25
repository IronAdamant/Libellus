# Libellus: LLM development log

## Project overview

**Libellus** 1.0.0 is a std-only Rust binary (`libellus`, `libellus-mcp`): a private
notepad and number ledger for each AI agent. It does not call an LLM.

**Guidelines:** No third-party crates. `[dependencies]`, `[dev-dependencies]` and
`[build-dependencies]` stay empty. No new TODO or FIXME markers in `src/`.

## Active context

- **Version:** 1.0.0, the first complete surface. Not tagged and not published.
- **Built:** note, get, resolve, ls, brief, promote, spend, ledger; `libellus hook`;
  `LIBELLUS_TOOLS`, `LIBELLUS_DISABLE`, `LIBELLUS_AGENT`.
- **Not in this tree:** release binaries, npm packages, an MCP Registry entry.
  Do not tag until Aron says so.

## Session log

---

## 1.0.0: the rest of the surface

**Date:** 2026-11-25

- `libellus_get` and `libellus_resolve`, with tests for a missing note, a corrupt
  file, a bad outcome, and a second resolve.
- `libellus_ledger` totals and breakdowns. `LIBELLUS_TOOLS` and `LIBELLUS_DISABLE`
  replace the AGENTS.md line; a typo falls back to core.
- `libellus_promote` writes a Relatum note without running Relatum. An open
  question, a missing store, a second promote, and a path that leaves the store
  are errors.
- `libellus hook` prints a brief snippet for a generic host, Claude Code, and
  Cursor. `LIBELLUS_AGENT` fills a required agent flag.
- Docs and the crate version updated to match the tools that exist.

---

## 0.1.0: scaffold

**Date:** 2026-11-25

- Started as the 8th tool. Built by Grok Bot from the design agreed with Aron, to be
  reviewed and finished in Claude Code.
- The foundations (JSON parser, MCP loop, tool table, atomic writes, path guards)
  were written to avoid the failure modes found across the seven earlier tools.
