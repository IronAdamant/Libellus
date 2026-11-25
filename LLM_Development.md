# Libellus: LLM development log

## Project overview

**Libellus** 0.1.0 is a std-only Rust binary (`libellus`, `libellus-mcp`): a private
notepad and number ledger for each AI agent. It does not call an LLM.

**Guidelines:** No third-party crates. `[dependencies]` stays empty. No new TODO or
FIXME markers in `src/`.

## Active context

- **Version:** 0.1.0, a scaffold. Nothing is tagged or published.
- **Built:** note, ls, brief, spend; strict store; hostile-input tests.
- **Next:** resolve, get, promote into Relatum, compaction hook wiring, ledger
  totals. See SPEC.md section 6.

## Session log

---

## 0.1.0: scaffold

**Date:** 2026-11-25

- Started as the 8th tool. Built by Grok Bot from the design agreed with Aron, to be
  reviewed and finished in Claude Code.
- The foundations (JSON parser, MCP loop, tool table, atomic writes, path guards)
  were written to avoid the failure modes found across the seven earlier tools.
