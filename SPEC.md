# Libellus format v0.1

Libellus is a private notepad and number ledger per agent. The files are the
product. The CLI and the MCP server are conveniences over them, and any agent
that can read and write files can use the store without either.

## 1. Layout

```
.libellus/
  agents/<agent>/notes/<id>.md       notes; <id> is one or two slug segments
  agents/<agent>/ledger.jsonl        the number ledger
```

- The store is the nearest `.libellus/` walking up from the working directory,
  or the folder named by `$LIBELLUS_DIR`. `libellus init` creates it. No tool
  creates a store by accident: with none found, calls fail and say so.
- `<agent>` and every id segment are slugs: `[a-z0-9][a-z0-9-]*`, at most 64
  bytes. An id has at most two segments (`retry/post-question`).
- On Unix the folders are created mode 0700 and files 0600.
- A path component that is a symlink is refused. A file that is not a note in
  the notes folder is an error.

## 2. Notes

```markdown
---
kind: question
status: open
summary: Does retry apply to POST?
agent: main
created: 2026-11-25T02:05:00Z
evidence: [trammel:plan-3/step-2, chisel:tests/net.rs]
tags: [net, retry]
---

The decision note says GET only, but the client retries every method.
```

Frontmatter is a flat subset of YAML (`key: value`, lists written `[a, b]`).

| Field | Required | Meaning |
|---|---|---|
| `kind` | yes | `decision`, `dead-end`, `question`, `finding`, `context` |
| `status` | yes | `open` or `resolved`. Default: open for a question, resolved otherwise |
| `summary` | yes | One line, up to 200 characters, no leading or trailing space |
| `agent` | yes | The owning agent |
| `created` | yes | UTC, `YYYY-MM-DDTHH:MM:SSZ` |
| `evidence` | no | Pointers, each `tool:reference` (see below) |
| `tags` | no | Slugs |
| `promoted` | no | Reserved for the promote step: the Relatum note id this became |

An unknown key, a duplicate key, a bad pointer, a bad tag, a missing field or an
unclosed frontmatter is an error naming the file. A note is never overwritten:
creating an id that exists fails.

Evidence pointers: `<tool>:<reference>`, the tool a slug, the reference up to 200
characters with no whitespace, commas or brackets. Libellus does not check that
the reference resolves. It is a pointer. `adsertio:<claim-id>` points at a claim
in Adsertio, which reads the actual evidence.

A note is a fact, not a document: keep the body under about 300 words.

## 3. Ledger

One JSON object per line, appended, never rewritten:

```json
{"ts":"2026-11-25T02:07:00Z","agent":"main","task":"retry","step":"plan","model":"x","tokens_in":1200,"tokens_out":300,"cost_micro_usd":15000,"note":"first pass"}
```

`step`, `model` and `note` are optional. The three numbers are whole numbers
(unsigned, up to 64 bits); `cost_micro_usd` is millionths of a US dollar. Sums use
checked arithmetic and fail on overflow. A line that is damaged, has an unknown
field, or a file that ends in a partial line fails every read and every append.

## 4. MCP tools (built)

All arguments are checked against the tool's schema. Unknown arguments, wrong
types, missing required arguments and values outside an `enum` are errors with
`isError: true`.

- `libellus_note` `{agent, kind, summary, body?, id?, status?, evidence?, tags?}`
  returns `{id, kind, status}`. Without `id`, one is made from the summary.
- `libellus_ls` `{agent, kind?, status?, limit?, bodies?}` returns
  `{notes, total}`, oldest first, the newest `limit` kept (default 50). Bodies
  are cut at 4000 characters with `truncated: true`.
- `libellus_brief` `{agent, max_items?}` returns `open_questions`,
  `recent_decisions`, `dead_ends`, `costs` and `text`, a short markdown version
  to paste back into context.
- `libellus_spend` `{agent, task, step?, model?, tokens_in?, tokens_out?,
  cost_micro_usd?, note?}` (at least one number) returns the task and agent
  totals.

Groups: `core` (note, ls, brief) is always on; `ledger` (spend) can be switched
off. A hidden tool's call returns `isError` naming AGENTS.md.

## 5. Protocol behaviour

Newline-delimited JSON-RPC 2.0 on stdio.

| Input | Reply |
|---|---|
| Invalid UTF-8, malformed JSON, nesting deeper than 64 | `-32700`, server continues |
| A line over 4 MiB | `-32600`, the line is skipped without being buffered |
| A batch (array) or a non-object | `-32600` |
| `id` that is not a string, number or null | `-32600` |
| Unknown method with an id | `-32601` |
| Unknown method without an id, or `notifications/*` | no reply |
| `tools/call` for an unknown tool | `-32602` |
| A tool failure | a result with `isError: true` |
| A tool that panics | a result with `isError: true`; the server continues |

## 6. Planned, not built

These are agreed in the design and left for the next stage.

- `libellus_resolve`: set a note's status to resolved and append a one-line
  outcome to its body. The file is replaced atomically.
- `libellus_get`: read one note by id.
- `libellus_promote`: the explicit step into Relatum. Creates a Relatum note
  (create-only, never overwriting) under a scope the caller names, in Relatum's
  format (`kind`, `summary`, `author`, `updated`, `tags`), and records its id in
  `promoted`. Kind mapping: decision to decision, context to context, finding to
  finding, dead-end to attempt. A question can be promoted once it is resolved,
  as a finding. Relatum stays shared, with no private scope.
- The compaction hook: run `libellus brief --agent <name>` from the host's
  session-start or pre-compaction hook so the brief is re-injected without the
  agent having to remember. The CLI already prints the brief as text; the hook
  wiring per host is untested.
- `libellus_ledger`: totals by task, step or agent. `libellus check` and the
  brief already read and sum the ledger.
- `LIBELLUS_TOOLS` and `LIBELLUS_DISABLE` environment variables.
- Release binaries, npm packages, an MCP Registry entry.
