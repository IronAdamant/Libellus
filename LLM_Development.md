# Libellus: LLM development log

## Project overview

**Libellus** 1.1.1 is a std-only Rust binary (`libellus`, `libellus-mcp`): a private
notepad and number ledger for each AI agent. It does not call an LLM. 1.0.0 and
1.1.0 are tagged releases.

**Guidelines:** No third-party crates. `[dependencies]`, `[dev-dependencies]` and
`[build-dependencies]` stay empty. No new TODO or FIXME markers in `src/`.

## Active context

- **Version:** 1.1.1 in the tree. 1.0.0 and 1.1.0 are GitHub releases.
  The release workflow publishes archives on `v*`. Not on crates.io or npm.
- **Built:** note, get, resolve, ls, brief, promote, spend, ledger; `libellus hook`;
  `LIBELLUS_TOOLS`, `LIBELLUS_DISABLE`, `LIBELLUS_AGENT`.
- **Not in this tree:** npm packages, an MCP Registry entry.

## Session log

---

## 1.1.1: Windows release zip

**Date:** 2026-11-25

- Crate version is 1.1.1. No behaviour changes.
- The Windows release archive is a real zip, and the release job checks the
  PK signature and lists the entries before upload.

---

## 1.1.0: faster reads and agent pages

**Date:** 2026-11-25

- Recorded a release baseline, then read note files in parallel and stopped
  spending from parsing the ledger twice. Before and after are in `docs/perf/`.
- `libellus_ls` gained `offset`. Errors for a missing note, a duplicate id, and
  a second resolve name the next call. The brief text ends with a `Next:` line.
- Not tagged.

---

## 1.0.0: release workflow

**Date:** 2026-11-25

- Added `.github/workflows/release.yml`. A `v*` tag builds Linux x64, macOS x64,
  macOS arm64, and Windows x64, writes SHA256SUMS, and opens a GitHub release
  whose notes are the matching CHANGELOG section. No npm and no crates.io.

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
