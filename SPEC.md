# Libellus format

Libellus is a private notepad and number ledger per agent. The files are the
product. The CLI and the MCP server are conveniences over them, and any agent
that can read and write files can use the store without either. Libellus 1.2.0
reads stores written by 1.0.0 and 1.1.2. Notes it writes add `seq`. A 1.1.2
binary rejects that key.

## 1. Layout

```
.libellus/
  agents/<agent>/notes/<id>.md       notes; <id> is one or two slug segments
  agents/<agent>/ledger.jsonl        the number ledger
```

- The store is the nearest `.libellus/` walking up from the working directory,
  or the folder named by `$LIBELLUS_DIR`. An empty `LIBELLUS_DIR` is treated
  as unset. `libellus init` creates it. A folder
  set as `LIBELLUS_DIR` with no `agents` directory is not a store: calls fail
  and say to run `libellus init`. No tool creates a store by accident.
- `<agent>` and every id segment are slugs: `[a-z0-9][a-z0-9-]*`, at most 64
  bytes. An id has at most two segments (`retry/post-question`). A Windows
  device name (`con`, `prn`, `aux`, `nul`, `com1` to `com9`, `lpt1` to `lpt9`)
  is refused as an agent name or id segment on every platform, so a store
  stays portable. A note file with such a name is skipped by `ls` and `brief`
  with a warning. An id made from a summary that would be one gets `-note`
  added (`con-note`).
- On Unix the folders are created mode 0700 and files 0600.
- A path component that is a symlink is refused, including `ledger.jsonl`,
  `notes/` and `archive/`. A `.libellus` found while walking up that is a
  symlink or a file is an error; the walk does not carry on to a parent store.
  A regular file that is not a note in the notes folder is an error. A fifo
  or other non-regular file is not opened: `ls` and `brief` skip it and warn,
  and `check` reports it.

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
A note saved with CRLF line endings is read as if it had LF. Reading does not
rewrite the file; the next resolve or promote writes it with LF.

| Field | Required | Meaning |
|---|---|---|
| `kind` | yes | `decision`, `dead-end`, `question`, `finding`, `context` |
| `status` | yes | `open` or `resolved`. Default: open for a question, resolved otherwise |
| `summary` | yes | One line, up to 200 characters, no leading or trailing space |
| `agent` | yes | The owning agent. A file whose agent is not the caller is an error |
| `created` | yes | UTC, `YYYY-MM-DDTHH:MM:SSZ`. Resolve and promote leave it unchanged |
| `seq` | no | Whole number. New notes get the next value for that agent. Older notes omit it |
| `superseded` | no | `true` when the note was marked replaced. Absent means it was not |
| `evidence` | no | Pointers, each `tool:reference` (see below) |
| `tags` | no | Slugs |
| `promoted` | no | Relatum note id written by `libellus_promote` |

An unknown key, a duplicate key, a bad pointer, a bad tag, a missing field, a
`created` value that is not `YYYY-MM-DDTHH:MM:SSZ`, or an unclosed frontmatter
is an error naming the file. A repeated tag is stored once. A note file over
1 MiB is an error naming the file. The `body` argument is at most 20000 bytes,
the same cap as every other string argument. Creating an id that exists fails.
`libellus_get`
returns the whole body of a note under that cap. `libellus_ls` cuts a body at
4000 characters and sets `truncated` on that note. A note whose `agent` field
does not match the folder is skipped by `ls` and `brief` with a warning, and
`check` reports it. `get` still refuses it. A note file whose name is not one
or two slugs, and anything in the notes folder that is not a regular file
(a fifo, for example), is skipped the same way. A symlink is still an error.

Notes with `seq` keep the order of that number. Notes without `seq` are
ordered by `created`, then by id. The two runs are merged by `created`:
inside one second, a note with `seq` is newer than a note without it. A
`seq` order that disagrees with `created` (a clock step, a hand-written note)
keeps the `seq` order and does not fail. `seq` comes from `agents/<agent>/seq`. A missing
counter starts at the highest `seq` already stored, or 0. A damaged counter
is an error and is not reset. Resolve and promote leave `seq` unchanged.

`libellus_resolve` sets `status` to `resolved` and appends one line to the
body, keeping the old body:

```
Outcome: No, GET only.
```

The outcome is one line, at most 200 characters, with no leading or trailing
space. A note that is already resolved is an error, and the file is not
changed, unless the call passes `supersede` or the outcome starts with
`SUPERSEDED:` or `REVERSED:`. That call sets `superseded: true`, appends the
outcome, and is allowed on a note that is already resolved. The replacement
is atomic.

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

`step`, `model` and `note` are optional, and are strings when present; any
other type is a corrupt line. The three numbers are whole numbers
(unsigned, up to 64 bits); `cost_micro_usd` is millionths of a US dollar. Sums use
checked arithmetic. A spend checks the new total before it appends, and does
not append when the total would overflow. Under the lock it uses
`ledger.meta` when that cache still matches the file, and otherwise reads
the ledger once. The returned count includes the line just written. A line that is
not JSON, or that has an unknown field, fails every read and every append.
A line whose `agent` is not the folder, whose `task` or `step` is not a slug,
or whose `ts` is not `YYYY-MM-DDTHH:MM:SSZ` is left out of the totals.
`check` names that line. A file that ends in a partial line is still read:
the torn line is not counted, `check` names it and says to finish or delete
it, and `spend` will not append until that is done. A symlink ledger, or a
ledger that is not a regular file, is refused for both read and append.
A ledger over 32 MiB, or a line over 64 KiB, is an error and is not read.
`spend` refuses before an append would pass 32 MiB, and the file is unchanged.
`libellus archive` renames `ledger.jsonl` to
`archive/ledger-YYYYMMDDTHHMMSSZ.jsonl` (digits, `T`, and `Z` only, so the
name is safe on Windows) and starts a fresh ledger. The old file is kept and
is not included in later totals. `check` reads archives and does not apply
the 32 MiB cap to them. A damaged archive fails `check`.

`brief` still returns the notes when a ledger cannot be read (over the cap,
a line over 64 KiB, a fifo, a symlink, or a corrupt line). Costs are then
empty and `ledger_error` names the file. `ledger`, `spend`, and `check` still
fail on that file. A damaged ledger is not repaired.

A failed `spend` write truncates its own partial line back off the file. A
kill during the write can still leave a torn line. The next `spend` will not
append until that line is finished or deleted. `brief` keeps working and
names the torn line.

`spend` keeps totals in `ledger.meta` beside the ledger. It uses that cache
only when the file length and the last line still match, so a later spend
does not parse the whole file. Per-task totals are kept for up to 64 tasks.
With more tasks, `spend` still reads the ledger to total that one task, and
it does not rewrite a row for every task. The cache is not the ledger. A hand
edit that keeps the same length and the same last line is not seen by `spend`
until `brief`, `ledger`, or `check` reads the file and rewrites the cache. A
damaged cache is ignored. A ledger that is intact but whose running
total overflows is reported by `ledger` and `brief` as `ledger_error` with the
line number in the file (left-out and torn lines count); totals stop before
that line. `ledger` keeps a left-out line warning beside the overflow,
joined with `; `. `check` and `spend` name the same line. `check` fails on
that ledger.

`libellus_ledger` returns `agents`, `entries`, `tokens_in`, `tokens_out` and
`cost_micro_usd`. With no `task`, it also returns `by_task`, every task,
sorted by name. With no `step`, it also returns `by_step`. A spend with no
step is keyed `""` in `by_step`. The brief's `by_task` is not this full map:
it keeps the newest tasks, up to `max_items`, and reports how many were left
out. Grouping walks each entry once.
With no `agent`, it sums every agent folder and names those agents. `task` and
`step` filters must be slugs. `LIBELLUS_AGENT` does not fill `agent` here:
omitting it means every agent.

## 4. Promote

`libellus_promote` copies one private note into Relatum. It does not run the
`relatum` program, and it does not create `.swarmnotes/`.

The Relatum store is `$SWARMNOTES_DIR` when that is set and non-empty, otherwise
the nearest `.swarmnotes/` walking up from the working directory. If neither
exists, the call errors and tells the caller to run `relatum init`.
`SWARMNOTES_DIR` is the host's explicit choice and may lead through a
symlink. A `.swarmnotes` found by walking that is a symlink or a file is
refused, and the walk does not carry on to a parent store.

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
`_?[a-z0-9][a-z0-9-]*`. `..`, an absolute path, a symlink inside the store,
and a Windows device name as a scope segment are refused.
The file is markdown with Relatum frontmatter (`kind`, `summary`, `author`,
`updated` as `YYYY-MM-DD`, `tags`), created with a hard link so an existing
file is never replaced. `author` is the agent name. The body is the Libellus
body, or the summary when the body is empty. Evidence pointers are appended
as `Evidence: ...` because Relatum has no evidence field. If that id is taken,
the last segment gains a suffix `-2`, `-3`, and so on. Relatum reads a value
shaped `[...]` as a list, so a summary that starts with `[` and ends with `]`
is written with a trailing `.` to stay a string.

Promote holds the same note lock as resolve, from reading the note to
rewriting it, so a concurrent resolve or second promote is never lost.

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
- `libellus_resolve` `{agent, id, outcome, supersede?}` returns the updated note.
  `supersede`, or an outcome that starts with `SUPERSEDED:` or `REVERSED:`,
  sets `superseded` and is allowed when the note is already resolved.
- `libellus_ls` `{agent, kind?, status?, limit?, offset?, before_id?, bodies?}`
  returns `{notes, total, offset, returned, truncated, next_offset, next_before_id}`.
  Notes are oldest first within the page. `limit` (default 50, minimum 1)
  keeps the newest page. `offset` 0 is the 1.0.0 page. `offset` counts back
  from the newest note that matches `kind` and `status`, or from the cursor
  when `before_id` is given. `before_id` is the stable cursor: only notes
  older in list order (`seq`, or `created` then id), so a note written
  between pages is not listed twice. The cursor is placed by that order, not
  by finding the note in the filtered list, so it still works after the note
  is resolved, filtered out, or deleted. `next_before_id` is
  `<id>@<created>@<seq>` (or `<id>@<created>` for a note without `seq`); a
  plain note id is also accepted while that note exists. `truncated` is true
  when another page remains. `next_offset` and `next_before_id` are null
  when it does not. `next_offset` is absolute: the `offset` to pass without
  `before_id` for the next older page, also on a page reached with
  `before_id`. Prefer `next_before_id` for the next call. `total` is the
  count that matches `kind` and `status`.
- `libellus_brief` `{agent, max_items?}` returns `open_questions`,
  `open_questions_total`, `recent_decisions`, `replaced_decisions`,
  `dead_ends`, `recent_findings`, `context`, `costs`, `truncated` and `text`.
  `max_items` (default 5, at most 50) caps every section, including open
  questions and task costs. Items are newest first. `text` is at most 8192
  bytes and says when it was cut. `context` is the newest context note, with
  the body cut at 500 characters. Replaced decisions are listed apart from
  current ones, with `(replaced)` in the text. Task costs are the newest
  spends, not the alphabetical prefix. A ledger read error sets `ledger_error`
  and does not drop the notes. Callers from 1.0.0 still find the old fields.
  The CLI prints `text`.
  `--host cursor` is a CLI flag only (it is not an MCP argument) and prints
  `{"additional_context": "<text>"}`. The text keeps the 1.0.0 headings and
  cost lines. A `Next:` line is added only when a question is still open or a
  decision or dead end has not been promoted. `text` is the short form (one
  line per item). The arrays are the same facts for a caller that parses
  fields. Both stay so a 1.0.0 caller keeps working.
- `libellus_check` `{}` reads every note and ledger. It fails when a file is
  damaged, a note is over 1 MiB, a ledger total overflows, a note's `agent`
  field does not match its folder, a note id cannot be opened, a ledger line
  was left out of the totals, a ledger ends in a partial line, or a stale
  lock could not be removed. `check` first deletes stale locks and temp files
  whose writer is gone. A lock with no finished pid line is stale after one
  second. A finished line that names a dead process is stale at once. A fresh
  lock that is not a pid line is left in place. Only the holder of the
  `.<name>.lock.break` guard removes a stale lock, and it judges the lock
  again while holding the guard, so a lock taken a moment later is never
  removed. A holder deletes its lock only while the file still holds the line
  it wrote. On Unix a pid is checked with `kill(pid, 0)`: EPERM is alive,
  ESRCH is dead. `ps` is not run. On Windows a pid cannot be checked and a
  lock is judged by its age. The success JSON lists
  `removed`.
- `libellus_promote` `{agent, id, scope}` returns `{id, promoted, relatum_kind, path}`.
- `libellus_spend` `{agent, task, step?, model?, tokens_in?, tokens_out?,
  cost_micro_usd?, note?}` (at least one number) returns the task and agent
  totals.
- `libellus_ledger` `{agent?, task?, step?}` returns the totals described above.
- `libellus_archive` `{agent}` moves that agent's ledger into `archive/` and
  starts a fresh file. It is in the ledger group.

`LIBELLUS_AGENT`, when set and non-empty, fills a required `agent` that the
caller omitted. An explicit `agent` wins.

Groups: `core` (note, get, resolve, ls, brief, promote, check) is always on; `ledger`
(spend, ledger, archive) can be switched off. A hidden tool's call returns `isError`.

The choice is one line in the project's AGENTS.md `agent-tools` block:

```text
libellus: disable ledger
libellus: tools core, ledger
```

If `LIBELLUS_TOOLS` or `LIBELLUS_DISABLE` is set to a non-empty value, that
pair replaces the file line. `LIBELLUS_TOOLS` is the allowlist and
`LIBELLUS_DISABLE` is applied after it. Group names are comma-separated and
case is ignored, in the file line too (prefix, verb and group names). With no
store yet, the AGENTS.md read is in the parent of `LIBELLUS_DIR` when that is
set, else the nearest folder walking up from the working directory that has
an AGENTS.md. An unknown group, an empty list, or disabling `core` falls
back to the core tools only and says why in `initialize` and in `tools/list`.

`libellus hook` is a CLI command, not an MCP tool. It prints a snippet whose
job is to run `libellus brief --agent NAME` at session start and after
compaction. `--host` is `generic` (the default), `claude`, or `cursor`. The
claude snippet is a SessionStart hook whose plain stdout is added to the
session, including after compaction (`compact`). The cursor snippet is a
sessionStart hook that prints `additional_context`. Cursor's preCompact hook
cannot put text back into the model. `hook` reads its flags with the same
rules as every other command: `--name=value` works, and a flag given twice
is an error.

`init`, `check`, `mcp`, `serve-mcp`, `help` and `--version` take no arguments.
An extra argument is a usage error (exit 2).

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
| A response from the client (`result` or `error`, no `method`) | no reply |
| An id with no `method`, `result` or `error` | `-32600` |
| `tools/call` for an unknown tool | `-32602` |
| A tool failure | a result with `isError: true` |
| A tool that panics | a result with `isError: true`; the server continues |

## 7. Not in this tree

A `v*` tag runs `.github/workflows/release.yml`, which publishes GitHub
release archives and SHA256SUMS. npm packages and an MCP Registry entry are
not part of this repository. Libellus does not price models, does not read
another agent's notes (that is Relatum, after promote), and does not resolve
an `adsertio:` pointer.
