# Libellus format

Libellus is a private notepad and number ledger per agent. The files are the
product. The CLI and the MCP server are conveniences over them, and any agent
that can read and write files can use the store without either. Libellus 1.4.0
reads notes and ledgers written by 1.0.0 to 1.3.1 (1.3.0 added the board,
section 5). Notes written since 1.2.0 have `seq`; a 1.1.2 binary rejects that
key. An older binary ignores `board/` and `board.html`.

## 1. Layout

```
.libellus/
  agents/<agent>/notes/<id>.md       notes; <id> is one or two slug segments
  agents/<agent>/seq                 the note counter
  agents/<agent>/ledger.jsonl        the number ledger
  board/<id>.md                      board tasks, shared by every agent
  board/seq                          the board counter
  board/archive/<project>/<id>.md    closed projects
  board.html                         the read-only page for people
  locks/<agent>.notes.lock           lock files (section 1.1), never deleted
  locks/<agent>.ledger.lock
  locks/<agent>.seq.lock
  locks/board.lock
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

### 1.1 Locking

Writers that read, check and then write hold an operating-system lock on
one file under `locks/`:

| Lock | Held by |
| --- | --- |
| `<agent>.notes.lock` | resolve (each item of a batch) and promote, from reading the note to rewriting it |
| `<agent>.ledger.lock` | spend, archive, and the locked ledger read of `libellus_ledger` and the brief |
| `<agent>.seq.lock` | taking note numbers for a note or a batch |
| `board.lock` | every board write, from the read to the last write and the page |

The lock is `flock(fd, LOCK_EX | LOCK_NB)` on Unix and `LockFileEx` with
`LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY` on the whole file on
Windows, on the lock file opened read-write (created when missing, 0600)
and kept open while held. A busy lock is retried with a sleep of 1 to 10 ms
for up to 8 seconds, then the call fails with `<path> is locked by another
libellus process. Retry shortly.` Any other error names the path. The
kernel releases the lock when the holder closes the file, exits or is
killed, so there is no stale lock, no pid or age in the file, and nothing to
clear by hand. Lock files are empty and are never deleted: removing a locked
file lets one writer lock the old file while another locks a new one. A
symlink or non-regular file at a lock path is refused. `check`, `doctor` and
the reaper treat `locks/` as part of the store and leave it alone.

Locks are advisory: an agent that edits the files by hand takes none.

Files from 1.3: 1.3 locked with a file `.<name>.lock` beside what it
guarded (with a `.<name>.lock.break` guard), created exclusive and deleted
on release. 1.4 never uses those names, so one left in a store blocks
nothing. `doctor` lists them under `stray_files`, and `check` (or `doctor
--fix`, or the brief's own clean-up) deletes them. Running 1.3 and 1.4
writers on one store at the same time is not supported: they take
different locks.

Network file systems: `flock` on NFS is emulated with byte-range locks or
local to one client on some setups, and SMB clients differ in how they map
it. Two writers on different machines may then both get a lock. `doctor`
has a `file_system` check that warns (and still passes) when the store is on
a network file system: on macOS a mount without `MNT_LOCAL` (autofs trigger
points aside), on Linux x86_64 and aarch64 an `f_type` of NFS, SMB, CIFS,
SMB2, 9P, AFS, Coda or Ceph, on Windows a UNC path or a drive that
`GetDriveTypeW` calls remote. Other platforms are not checked.

Temp files (`.<name>.tmp-<pid>-<n>`) are judged by age only: one older than
an hour is abandoned and `check` removes it. A write holds its temp for
milliseconds. A writer paused for longer (a laptop asleep mid-write) only
loses that write: its rename or link fails with an error and nothing else
changes, since temp names are never reused.

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
`libellus_ledger` with `archive: true` (CLI `libellus ledger --agent A
--archive`) renames `ledger.jsonl` to
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

Promote holds the agent's notes lock (section 1.1), as resolve does, from
reading the note to rewriting it, so a concurrent resolve or second promote
is never lost.

Then `promoted` on the Libellus note is set to that Relatum id, atomically.
If the Relatum write succeeded and this update fails, the error names both
paths. A second promote of a note that already has `promoted` is an error
naming that id, and it does not write another Relatum file.

## 5. Board

Notes are private per agent. The board is the one deliberate shared part of
the store: a tree of tasks that every agent in the store reads and changes.
A main agent plans a project and assigns subtrees to sub-agents (`owner`);
each agent ticks off its own tasks; parents close by themselves. Trammel
plans code changes in dependency order and verifies them. The board is the
agents' own working task list.

### Task files

One file per task, `board/<id>.md`. `<id>` is one slug (not a Windows device
name), unique on the live board. A task has no body.

```markdown
---
title: Write the docs
status: doing
parent: release-1-3
owner: sub-2
created: 2026-11-26T01:00:00Z
updated: 2026-11-26T02:10:00Z
updated_by: sub-2
seq: 14
outcome: README and SPEC done
evidence: [adsertio:claim-7, trammel:plan-3/step-2]
---
```

| Field | Required | Meaning |
|---|---|---|
| `title` | yes | One line, up to 200 characters, no leading or trailing space |
| `status` | yes | `todo`, `doing`, `blocked`, `done` or `dropped`. The first three are open |
| `parent` | yes | A task id, or empty for a top-level project |
| `owner` | yes | The agent that does it |
| `created`, `updated` | yes | UTC, `YYYY-MM-DDTHH:MM:SSZ` |
| `updated_by` | yes | The agent that made the last change |
| `seq` | yes | Creation order from `board/seq`. Siblings are listed by it |
| `closed`, `closed_by` | when closed | Set exactly when the status is `done` or `dropped`. `closed_by` is an agent, or `children` |
| `outcome` | no | One line, like a resolve outcome |
| `evidence` | no | Pointers, `tool:reference`, as in notes |

Parsing is as strict as a note: an unknown or repeated key, a bad value, a
missing field, an open status with `closed` set, or text after the
frontmatter is an error naming the file. A task file over 64 KiB is an
error. `children` cannot be an owner or a writer. The tree is at most 6
levels deep (a project is level 1). A parent loop or a deeper chain, which
only a hand edit makes, is an error. A task whose parent is missing (a write
cut short) is shown as a top-level task, and `check` names it. The counter
follows the note counter: under the board lock, a missing `board/seq`
starts at the highest `seq` on the board, and a damaged one is an error.

### Rules

- A task cannot be `done` while it has an open child. The error names the
  open children.
- Dropping a task drops its open descendants (`closed_by` is the agent).
  Its done descendants stay done.
- Closing: when a task becomes done or dropped, or is removed or moved away,
  and every child of its parent is done or dropped with at least one done,
  the parent becomes `done` with `closed_by: children`. This repeats up to
  the project. All children dropped closes nothing.
- Reopening: an open task (set back to todo, doing or blocked, added, or
  moved) under a parent with `closed_by: children` reopens that parent as
  `doing`, and so on up. A parent closed by an agent is not reopened: that
  call is an error and says to reopen the parent first.
- Archive: when a project (a top-level task) closes, its whole subtree moves
  to `board/archive/<project>/`, or `<project>-2` and so on when that
  folder is taken. Archived tasks keep their files for history and the
  page. They are never in the brief, cannot be changed, and their ids can
  be used again on the live board.
- Any agent may change any task. `updated_by` records who.

### libellus_plan

`{agent, ops?, outline?, parent?}`. `ops` is up to 100 operations, applied
in order:

```json
{"add": {"title": "Write docs", "id": "docs", "parent": "rel", "owner": "sub-2", "status": "todo"}}
{"set": {"id": "docs", "status": "done", "title": "...", "parent": "", "owner": "main", "outcome": "...", "evidence": ["adsertio:claim-7"]}}
{"remove": "typo-task"}
```

`add` needs `title`. Without `id`, the id is made from the title, with
`-2`, `-3` and so on when taken. `owner` defaults to the caller, `status` to
`todo`. `set` needs `id` and at least one other field. `parent: ""` makes a
task a project; a move may not put a task under itself or past depth 6. An
empty `outcome` clears it; `evidence` replaces the list. `remove` is only
for a `todo` task with no children (a mistake); otherwise set `dropped`.
Each op has exactly one of `add`, `set`, `remove`.

`outline` adds a tree in one call, after the ops. With `parent`, its top
lines go under that task; otherwise they are new projects. Grammar:

```text
outline = line *( "\n" line )
line    = blank / indent "- [" box "] " title
indent  = *( "  " ) / *( "\t" )      ; one level per two spaces or per tab
box     = " " / "x" / "~" / "!" / "-"  ; todo, done, doing, blocked, dropped
```

Blank lines are skipped and CRLF is read as LF. The first line is not
indented, and a line is at most one level below the line above. Anything
else is an error naming the line: an odd number of spaces, tabs and spaces
mixed, any other box (`[X]` too), another bullet, no space after the box,
an empty or over-long title, or a seventh level. An outline creates at most
200 tasks. An `[x]` parent with an open child is an error.

The call takes the board lock, reads the board, applies every op to a copy
in memory, settles reopening, checks that no closed task has an open child,
settles closing, and only then writes. Any error means no file changes, and
the error names the op (`ops[3] (set "x")`) or outline line. Writes are new
and changed task files (`overwrite = false` for a new task), then removed
files, then archive moves (renames). A kill during the writes can leave some
written: each file is whole, `check` names what is out of step (an open
child under a closed parent, a closed project still on the live board, an
orphan), and the next `plan` call archives a closed project left behind.

The reply lists only what changed: `added` (ids, in order, so an outline's
ids are known), `updated`, `removed`, `closed_by_children`, `reopened`,
`dropped`, `archived` (`{project, folder}`), `files_written`, and `html` (the
page path) or `html_error`. A failed page write does not undo the board
write.

Locking: one board lock, `locks/board.lock`, the same operating-system
lock as notes (section 1.1), held from the read to the last write and the
page. Not
a lock per task: an op reaches ancestors (closing and reopening), descendants
(dropping) and whole projects (archive), and which tasks those are is only
known after the read. One lock makes read, check and write one step, so two
agents ticking the last two children at once close the parent exactly once
and lose no update. Board writes queue for milliseconds. Reads take no lock;
every file is replaced whole, so a reader sees each task before or after a
change.

### libellus_board

`{agent, owner?, project?, open_only?, include_archived?}` returns `text`,
the tree as checkbox markdown (`- [~] id: title`, two spaces per level,
` @owner` when the owner is not the caller, ` -> outcome` when set),
`counts` per status for the live board, and `shown`. `open_only` defaults
to true. `owner` keeps that agent's tasks and their parents. `project` keeps
one project; a project that is not live is an error unless
`include_archived` is set, which appends each archived project as
`archived <folder>:` and its tree. `text` is cut at 16 KiB on a line
boundary and says so.

### In the brief

`## Board` comes after `## Current state`. It lists the caller's open tasks
that have no open children (a parent with open children closes by itself),
`doing` first, then `blocked`, then `todo`, each as
`- [~] project > task > sub: Title`, capped by `max_items`, then one line of
counts for the whole board and the number of archived projects. The JSON
has `board` (`tasks`, `open_mine`, `counts`, `archived_projects`). A board
that cannot be read sets `board_error` and keeps the notes. With the `board`
group off, the section is left out.

### board.html

After every board write, under the board lock, `.libellus/board.html` is
written again (atomically). `libellus board --html [--out PATH]` writes it on
demand. One file, inline CSS, no scripts, no links to anything outside it:
it works offline, needs no JavaScript (collapsing is `<details>`), follows
`prefers-color-scheme` and fits a phone. It has a banner saying the page is
read-only and how to ask for a change ("ask your agent, for example: review
task <id>"), each project's progress (done/total over the tasks under it,
not counting the project itself or dropped tasks), a kanban of `todo`,
`doing`, `blocked` and `done` tasks (projects are in the progress list, not
the columns) with dropped tasks collapsed below, the tree
with disabled checkboxes, each agent's open questions (id and summary only),
archived projects collapsed at the bottom, and the generation time and store
path in the footer. Each task shows id, title, owner, status, updated time
and writer, outcome and evidence. Every value is HTML-escaped (`&`, `<`,
`>`, `"`, `'`).

Cost: every board read is O(tasks): each file is read once (in parallel
above 128 files) and the tree is one index. A write also reads the archive
and every agent's notes for the page.

## 6. MCP tools

All arguments are checked against the tool's schema. Unknown arguments, wrong
types, missing required arguments and values outside an `enum` are errors with
`isError: true`.

- `libellus_note` `{agent, kind, summary, body?, id?, status?, evidence?, tags?}`
  returns `{id, kind, status}`. Without `id`, one is made from the summary.
  Or `{agent, notes}`: up to 50 notes, each `{kind, summary, body?, id?,
  status?, evidence?, tags?}`, never mixed with the single-note fields. The
  whole batch is checked first (fields, a taken id, an id twice in the
  batch); any bad item means nothing is written, and the error names every
  bad item (`notes[2]: ...`). Then the notes are written in order with
  `overwrite = false` and `seq` numbers taken in one locked step. A write
  that fails part way stops the batch and the error names the notes already
  written. The reply is `{written, ids}`.
- `libellus_get` `{agent, id}` returns the note, including `body`, `evidence`
  and `tags`. Missing, corrupt, a symlink, or a mismatched `agent` is an error.
- `libellus_resolve` `{agent, id, outcome, supersede?}` returns the updated note.
  Or `{agent, items}`: up to 50 `{id, outcome, supersede?}`, checked whole
  first (a missing note, an already resolved note without supersede, an id
  twice), then resolved in order, each under the agent's notes lock. The reply is
  `{resolved, notes: [{id, status, superseded?}]}`.
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
- `libellus_brief` `{agent, max_items?}` returns `board` (section 5), `open_questions`,
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
- `libellus check` (a CLI command; over MCP the same scan is
  `libellus_doctor`'s `store_files` check) reads every note and ledger. It fails when a file is
  damaged, a note is over 1 MiB, a ledger total overflows, a note's `agent`
  field does not match its folder, a note id cannot be opened, a ledger line
  was left out of the totals, or a ledger ends in a partial line. `check`
  first deletes temp files over an hour old and lock files left by 1.3
  (section 1.1); the lock files in `locks/` are never touched. The success
  JSON lists `removed`.
  `check` also reads the board and its archive (section 5).
- `libellus_doctor` `{fix?}` returns `{ok, checks: [{name, ok, detail,
  fix}]}`: `version` (and platform), `agent` (`LIBELLUS_AGENT`), `store`
  (where, and whether `LIBELLUS_DIR` chose it), `writable` (a probe file
  made and removed with `write_atomic`), `stray_files` (temp files over an
  hour old and 1.3 lock files), `file_system` (warns, and passes, when the
  store is on a network file system, section 1.1), `store_files` (the
  `check` scan, read-only),
  `tool_groups` (the AGENTS.md line or the environment variables),
  `mcp_binary` (`libellus-mcp` next to the binary), `relatum` (the promote
  store; none found is ok and says so) and, on Windows, `path_length` (the
  store path plus about 100 characters, against the 260 limit). Each
  failing check has a `fix`. `fix: true` removes only those temp files and
  1.3 lock files, as `check` does. The CLI exits 1 when `ok` is false.
- `libellus_promote` `{agent, id, scope}` returns `{id, promoted, relatum_kind, path}`.
- `libellus_spend` `{agent, task, step?, model?, tokens_in?, tokens_out?,
  cost_micro_usd?, note?}` (at least one number) returns the task and agent
  totals.
- `libellus_ledger` `{agent?, task?, step?, archive?}` returns the totals
  described above. With `archive: true` it instead moves that agent's ledger
  into `archive/` and starts a fresh file, returning `{agent, archived,
  bytes}`. Archive needs `agent` (or `LIBELLUS_AGENT`) and takes no `task`
  or `step`.
- `libellus_plan` and `libellus_board`: section 5. They are the board group.

A batch argument (`notes`, `items`, `ops`) is a JSON array of objects whose
keys are checked like top-level arguments: an unknown key, a wrong type, a
missing required key or a value outside an `enum` is an error naming the
item. The schema gives each item's properties (types, enums, required keys)
and `maxItems`, but not a description per field: a field means what the
same field means in the single form.

The definitions are kept small because a host sends them to the model on
every turn. `tests/surface.rs` fails when the compact JSON of the
`tools/list` array, every group on, passes 8,500 bytes, or the server
`instructions` reach 600 bytes. On the CLI it
is `--notes '<json>'` or `--notes-file PATH` (also `--items-file`,
`--ops-file` and `--outline-file`); the file is at most 4 MiB.

`LIBELLUS_AGENT`, when set and non-empty, fills a required `agent` that the
caller omitted. An explicit `agent` wins.

Groups: `core` (note, get, resolve, ls, brief, promote, doctor) is
always on; `ledger` (spend, ledger) and `board` (plan, board) can be
switched off. A hidden tool's call returns `isError`.

The choice is one line in the project's AGENTS.md `agent-tools` block:

```text
libellus: disable ledger
libellus: tools core, ledger
libellus: disable board
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
job is to put the brief into the session at start and after compaction.
`--host` is `generic` (the default), `claude`, or `cursor`. `generic` prints
`libellus brief --agent NAME`. The cursor snippet is a sessionStart hook that
prints `additional_context`; Cursor's preCompact hook cannot put text back
into the model. `hook` reads its flags with the same rules as every other
command: `--name=value` works, and a flag given twice is an error.

The claude snippet has two Claude Code hooks, both in exec form (`command`
plus an `args` array), so no shell parses them and they run the same under
cmd, PowerShell and Git Bash:

- `SessionStart`, matcher `startup|resume|compact`, runs
  `libellus hook-run session-start --agent NAME`, which prints
  `{"hookSpecificOutput": {"hookEventName": "SessionStart",
  "additionalContext": <brief text>}}`.
- `SubagentStart` runs `libellus hook-run subagent-start`. It reads the hook
  JSON on stdin (at most 1 MiB), takes `agent_type` and `agent_id`, and
  names the sub-agent `<agent_type>-<id>`: the type made a slug (at most 40
  characters, `sub` when empty) and the last 8 letters and digits of
  `agent_id` after any `agent-` prefix, lowercased (`Explore` and
  `agent-abc123` give `explore-abc123`). It prints the same JSON with
  `"hookEventName": "SubagentStart"` and a context that starts with one line
  telling the sub-agent its Libellus name, to pass it as `agent`, and that
  its tasks are on the board, followed by that agent's brief. If the prompt
  that started the sub-agent names another Libellus agent (`sub-2`), the
  sub-agent uses that name and calls `libellus_brief` with it.

`hook-run` always exits 0 so a bad store or input never stops a session:
on malformed stdin, a missing agent or a store error it prints nothing on
stdout and one line on stderr. There is no SubagentStop hook: its output
does not reach the parent agent.

`libellus doctor [--fix]` prints the doctor JSON and exits 1 when a check
fails. `libellus board --html [--out PATH]` writes the page and takes no
other flag.

`init`, `check`, `mcp`, `serve-mcp`, `help` and `--version` take no arguments.
An extra argument is a usage error (exit 2).

## 7. Protocol behaviour

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

## 8. Not in this tree

A `v*` tag runs `.github/workflows/release.yml`, which publishes GitHub
release archives and SHA256SUMS. npm packages and an MCP Registry entry are
not part of this repository. Libellus does not price models, gives an agent
no tool to read another agent's notes (that is Relatum, after promote; only
the people's page lists each agent's open questions), and does not resolve
an `adsertio:` pointer. The page is read-only: there is no server and no
form.
