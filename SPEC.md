# Libellus format

Libellus is a private notepad and number ledger per agent. The files are the
product. The CLI and the MCP server are conveniences over them, and any agent
that can read and write files can use the store without either. This is the
format Libellus 1.0.0 reads and writes.

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
| `agent` | yes | The owning agent. A file whose agent is not the caller is an error |
| `created` | yes | UTC, `YYYY-MM-DDTHH:MM:SSZ`. Resolve and promote leave it unchanged |
| `evidence` | no | Pointers, each `tool:reference` (see below) |
| `tags` | no | Slugs |
| `promoted` | no | Relatum note id written by `libellus_promote` |

An unknown key, a duplicate key, a bad pointer, a bad tag, a missing field or an
unclosed frontmatter is an error naming the file. Creating an id that exists
fails. `libellus_get` returns the whole body. `libellus_ls` cuts a body at
4000 characters and sets `truncated`.

`libellus_resolve` sets `status` to `resolved` and appends one line to the
body, keeping the old body:

```
Outcome: No, GET only.
```

The outcome is one line, at most 200 characters, with no leading or trailing
space. A note that is already resolved is an error, and the file is not
changed. The replacement is atomic.

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
A damaged ledger is not repaired.

`libellus_ledger` returns `agents`, `entries`, `tokens_in`, `tokens_out` and
`cost_micro_usd`. With no `task`, it also returns `by_task`. With no `step`,
it also returns `by_step`. A spend with no step is keyed `""` in `by_step`.
With no `agent`, it sums every agent folder and names those agents. `task` and
`step` filters must be slugs. `LIBELLUS_AGENT` does not fill `agent` here:
omitting it means every agent.

## 4. Promote

`libellus_promote` copies one private note into Relatum. It does not run the
`relatum` program, and it does not create `.swarmnotes/`.

The Relatum store is `$SWARMNOTES_DIR` when that is set and non-empty, otherwise
the nearest `.swarmnotes/` walking up from the working directory. If neither
exists, the call errors and tells the caller to run `relatum init`.

Kind mapping:

| Libellus kind | Relatum kind |
|---|---|
| `decision` | `decision` |
| `context` | `context` |
| `finding` | `finding` |
| `dead-end` | `attempt` |
| `question`, once `resolved` | `finding` |

An open question is refused. Other kinds may be promoted while open.

The new id is `<scope>/<libellus-id>`. Each segment matches Relatum's rule
`_?[a-z0-9][a-z0-9-]*`. `..`, an absolute path, and a symlink are refused.
The file is markdown with Relatum frontmatter (`kind`, `summary`, `author`,
`updated` as `YYYY-MM-DD`, `tags`), created with a hard link so an existing
file is never replaced. `author` is the agent name. The body is the Libellus
body, or the summary when the body is empty. Evidence pointers are appended
as `Evidence: ...` because Relatum has no evidence field. If that id is taken,
the last segment gains a suffix `-2`, `-3`, and so on.

Then `promoted` on the Libellus note is set to that Relatum id, atomically.
If the Relatum write succeeded and this update fails, the error names both
paths. A second promote of a note that already has `promoted` is an error
naming that id, and it does not write another Relatum file.

## 5. MCP tools

All arguments are checked against the tool's schema. Unknown arguments, wrong
types, missing required arguments and values outside an `enum` are errors with
`isError: true`.

- `libellus_note` `{agent, kind, summary, body?, id?, status?, evidence?, tags?}`
  returns `{id, kind, status}`. Without `id`, one is made from the summary.
- `libellus_get` `{agent, id}` returns the note, including `body`, `evidence`
  and `tags`. Missing, corrupt, a symlink, or a mismatched `agent` is an error.
- `libellus_resolve` `{agent, id, outcome}` returns the updated note.
- `libellus_ls` `{agent, kind?, status?, limit?, bodies?}` returns
  `{notes, total}`, oldest first, the newest `limit` kept (default 50).
- `libellus_brief` `{agent, max_items?, host?}` returns `open_questions`,
  `recent_decisions`, `dead_ends`, `costs` and `text`. The CLI prints `text`.
  `host: cursor` makes the CLI print `{"additional_context": "<text>"}` instead.
  The default text is unchanged.
- `libellus_promote` `{agent, id, scope}` returns `{id, promoted, relatum_kind, path}`.
- `libellus_spend` `{agent, task, step?, model?, tokens_in?, tokens_out?,
  cost_micro_usd?, note?}` (at least one number) returns the task and agent
  totals.
- `libellus_ledger` `{agent?, task?, step?}` returns the totals described above.

`LIBELLUS_AGENT`, when set and non-empty, fills a required `agent` that the
caller omitted. An explicit `agent` wins.

Groups: `core` (note, get, resolve, ls, brief, promote) is always on; `ledger`
(spend, ledger) can be switched off. A hidden tool's call returns `isError`.

The choice is one line in the project's AGENTS.md `agent-tools` block:

```text
libellus: disable ledger
libellus: tools core, ledger
```

If `LIBELLUS_TOOLS` or `LIBELLUS_DISABLE` is set to a non-empty value, that
pair replaces the file line. `LIBELLUS_TOOLS` is the allowlist and
`LIBELLUS_DISABLE` is applied after it. Group names are comma-separated and
case is ignored. An unknown group, an empty list, or disabling `core` falls
back to the core tools only and says why in `initialize` and in `tools/list`.

`libellus hook` is a CLI command, not an MCP tool. It prints a snippet whose
job is to run `libellus brief --agent NAME` at session start and after
compaction. `--host` is `generic` (the default), `claude`, or `cursor`. The
claude snippet is a SessionStart hook whose plain stdout is added to the
session, including after compaction (`compact`). The cursor snippet is a
sessionStart hook that prints `additional_context`. Cursor's preCompact hook
cannot put text back into the model.

## 6. Protocol behaviour

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

## 7. Not in this tree

A `v*` tag runs `.github/workflows/release.yml`, which publishes GitHub
release archives and SHA256SUMS. npm packages and an MCP Registry entry are
not part of this repository. Libellus does not price models, does not read
another agent's notes (that is Relatum, after promote), and does not resolve
an `adsertio:` pointer.
