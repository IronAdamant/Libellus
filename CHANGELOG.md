# Changelog

## 1.4.0

Operating-system file locks. A lock was a file made with `create_new` that
held a pid, a time and a token, so a writer that crashed left it behind and
Libellus had to guess when it was stale: a dead pid (which pid reuse and
separate pid namespaces get wrong), an age over 30 seconds (which a laptop
asleep for longer, or clock skew on a network drive, gets wrong), a bare
file after one second, and a `.break` guard. Now every lock is `flock` on
Unix and `LockFileEx` on Windows, on a lock file kept open while held. The
kernel releases it when the holder exits or is killed, so a crashed writer
blocks nobody and there is nothing to guess. Taking and releasing a lock went
from about 5 ms (a synced file create and delete) to about 40 us.

Breaking changes (there are no users yet):

- Lock files live in `.libellus/locks/` (`<agent>.notes.lock`,
  `<agent>.ledger.lock`, `<agent>.seq.lock`, `board.lock`) and are never
  deleted. Resolve and promote take one lock per agent instead of one per
  note.
- Running 1.3 and 1.4 writers on one store at the same time is not
  supported: they take different locks. Stop every 1.3 CLI, MCP server and
  hook before the first 1.4 write. A `.<name>.lock` or `.<name>.lock.break`
  file left by 1.3 blocks nothing; `check`, `doctor --fix` and the brief
  delete it, and `doctor` lists it under `stray_files` until then.
- There are no stale locks, so `check` no longer reports one and `doctor
  --fix` no longer removes one. The busy error is `<path> is locked by
  another libellus process. Retry shortly.`
- Temp files are reaped by age only: one over an hour old is removed. The pid
  in a temp name is no longer checked, so no pid is ever checked.
- Promote with a read-only notes folder now fails at the note rewrite (the
  error names the Relatum file it wrote) instead of at the lock.

Added:

- `doctor` has a `file_system` check. It warns, and passes, when the store is
  on a network file system, where `flock` may be emulated or not shared
  between machines: macOS `statfs` without `MNT_LOCAL`, a Linux NFS, SMB,
  CIFS, 9P, AFS, Coda or Ceph `f_type` (x86_64 and aarch64), a Windows UNC
  path or remote drive.
- `src/oslock.rs`, std only, written to be copied into another crate.
- `examples/perf.rs` times a lock acquire and release.

Removed: `fsx::Lock`, `fsx::break_stale_lock`, `fsx::lock_is_stale`,
`fsx::stale_lock_notes`, `fsx::pid_alive` (and its `kill(pid, 0)` call), the
lock token, `LOCK_STALE`, `LOCK_BARE` and the `.break` guard.

## 1.3.1

Smaller tool definitions. A host sends `tools/list` to the model on every
turn, so its size is paid on every turn. With every group on, the compact
`tools` array went from about 14,600 bytes to 8,386, and the server
`instructions` from 1,006 bytes to 536. Stores from 1.3.0 are read unchanged.

Breaking changes (there are no users yet):

- `libellus_archive` is merged into `libellus_ledger`: pass `archive: true`
  with `agent` (or `LIBELLUS_AGENT`). On the CLI, `libellus archive --agent A`
  is now `libellus ledger --agent A --archive`. The full-ledger error and the
  brief say so.
- `libellus_check` is no longer an MCP tool: `libellus_doctor` already runs
  the same scan as its `store_files` check, and `fix: true` clears stale
  locks and temp files as `check` did. The `libellus check` CLI command is
  unchanged.
- Descriptions are short: what the tool does, when to call it, and the one
  thing that trips an agent up. The long explanations are in README.md and
  SPEC.md. Batch item schemas (`notes`, `items`, `ops`) keep their types,
  enums, required keys and `maxItems`, but no longer repeat a description
  for each field.

Fixed:

- The schemas of the `add` and `set` ops in `libellus_plan` had `"type"`
  twice, which a strict JSON parser rejects.

Added:

- `tests/surface.rs` starts `libellus-mcp`, lists the tools with every group
  on, and fails when the array passes 8,500 bytes or the instructions reach
  600 bytes.

## 1.3.0

Notes and ledgers from 1.2.2 are read unchanged. New: batches, the shared
board, its read-only page, `doctor`, and Claude Code sub-agent hooks.

Breaking changes (there are no users yet):

- `libellus_note` no longer requires `kind` and `summary` in its schema:
  they are required only when `notes` is not given. Mixing `notes` with the
  single-note fields is an error.
- `libellus_resolve` likewise no longer requires `id` and `outcome` in its
  schema: they are required only when `items` is not given.
- A list argument with several bad items now names every bad item, prefixed
  `nothing written:`, instead of the first one only.
- `libellus hook --host claude` prints a different snippet: exec form
  (`command` plus `args`), matcher `startup|resume|compact` (no longer
  `clear`), the command `libellus hook-run session-start --agent NAME`
  (which prints `hookSpecificOutput` JSON, not plain text), and a new
  `SubagentStart` hook. Paste it again.
- `check` reports every damaged note or ledger in one error instead of
  stopping at the first, and now also fails on a damaged board.
- The brief has a `## Board` section after `## Current state`, and its
  `Next:` line can say "tick off a task with libellus_plan".
- New tool group `board`, on by default. A project whose AGENTS.md line is
  `libellus: tools core, ledger` does not get it.

Added:

- Batches. `libellus_note` takes `notes` (up to 50) and `libellus_resolve`
  takes `items` (up to 50 of `{id, outcome, supersede?}`). The whole batch
  is checked first; one bad item writes nothing, and the error names every
  bad item. Notes take their `seq` numbers in one locked step. A write that
  fails part way names what was written. On the CLI: `--notes '<json>'`,
  `--notes-file PATH`, `--items`, `--items-file`. Batch items are checked
  against a schema like top-level arguments (unknown keys are errors), and
  the MCP schema lists each item's properties and `maxItems`.
- The board, `.libellus/board/<id>.md`: one task tree shared by every agent
  in the store. Statuses todo, doing, blocked, done, dropped. Depth at most
  6. A parent closes by itself (`closed_by: children`) when its children are
  all done or dropped with one done, up to the project; a closed project
  moves to `board/archive/<project>/`. Dropping drops open descendants.
  Reopening a child reopens parents closed by children. A task cannot be
  done while a child is open.
- `libellus_plan`: up to 100 `add`, `set` and `remove` ops, or a markdown
  checkbox `outline` (optionally under `parent`), in one all-or-nothing call
  under one board lock. The reply lists what changed and every cascade.
- `libellus_board`: the board as checkbox markdown with counts, filtered by
  `owner`, `project`, `open_only` (default true) and `include_archived`.
- `.libellus/board.html`, rewritten after every board write, and
  `libellus board --html [--out PATH]`: a self-contained, read-only page
  (kanban, tree, progress per project, each agent's open questions,
  archived projects; light and dark; no scripts; every value escaped).
- `libellus doctor [--fix]` and `libellus_doctor` (core): diagnostics with a
  fix for each failing check. Exit 1 when a check fails. `--fix` only clears
  abandoned locks and temp files.
- `libellus hook-run session-start --agent NAME` and
  `libellus hook-run subagent-start` for Claude Code. A sub-agent is named
  `<agent_type>-<last 8 of agent_id>` and gets its brief. Bad input exits 0
  with a note on stderr, so a session never breaks.
- `examples/perf.rs` times the board at 5,000 tasks; numbers in
  `docs/perf/1.3.0.md`.

## 1.2.2

Same store format as 1.2.1. Fixes from the fifth review.

- The line named for a ledger total overflow counted only the lines that
  were added up, so a ledger with left-out lines named the wrong line.
  `ledger`, `brief`, `check` and `spend` now name the line in the file, from
  one shared helper. `ledger` keeps a left-out line warning beside the
  overflow instead of replacing it.
- The `agent-tools` line was case-sensitive. `libellus: disable Ledger` now
  disables the ledger, as the SPEC said.
- `ls --before_id` failed once the cursor note no longer matched the filter
  (resolved, with `--status open`) or was deleted. The cursor is placed by
  the list order. `next_before_id` is now `<id>@<created>@<seq>`, which still
  works after the note is deleted; a plain id is still accepted while the
  note exists.
- `next_offset` was wrong on a page reached with `before_id`. It is now
  absolute: the offset to pass without `before_id`.
- `init`, `check`, `mcp`, `serve-mcp`, `help` and `--version` ignored extra
  arguments. They are usage errors now.
- `libellus hook --agent=main` failed, and a repeated `--host` was silently
  last-wins. `hook` uses the same flag parser as the other commands.
- An empty `LIBELLUS_DIR` was used as a path. It is treated as unset.
- Before `libellus init`, the server ignored the AGENTS.md groups line and
  listed every tool. It now reads the nearest AGENTS.md walking up.
- A note saved with CRLF line endings was reported as corrupt and broke `ls`
  and `brief`. It is read as LF. The file is not rewritten on read.
- Lock liveness ran `ps -p`. A `ps` without `-p` (BusyBox), or one that
  failed, made every holder look dead, so live locks were broken. On Unix the
  check is now `kill(pid, 0)`, and no process is started while waiting.
- A `ledger.meta` without `tasks_complete` was trusted for task totals. It
  now counts as incomplete, so the task is totalled from the ledger.
- A symlinked `.swarmnotes` found by walking up was followed. It is refused,
  as the SPEC said. `SWARMNOTES_DIR` may still lead through a symlink.
- Windows device names (`con`, `nul`, `aux`, `prn`, `com1`-`com9`,
  `lpt1`-`lpt9`) are refused as agent names, note ids and Relatum scopes on
  every platform. An id made from a summary such as "Con" becomes `con-note`.
- A JSON-RPC response from the client got an error reply. It gets none.
- The brief said "1 ledger entries"; it now says "1 ledger entry".
- The crate doc named a `basis.md` that does not exist.

## 1.2.1

Same store format as 1.2.0. Fixes from the fourth review.

- Breaking a stale lock let several writers hold it at once: two contenders
  could each judge the old lock stale, and the later one deleted the lock the
  first had just taken. After a killed writer, notes could share a `seq` and
  resolve, spend and archive lost their exclusion. Only the holder of a
  `.break` guard removes a stale lock now, after judging it again. Each lock
  line carries a token, and a holder removes its lock only while the file
  still holds that line. `check` and `brief` reap through the same path.
- Promote now holds the note lock. A resolve running beside it was lost, and
  two promotes could both write a Relatum note.
- `ls`, `brief` and `check` panicked when notes with and without `seq` had
  `created` times out of order. Notes are merged instead of sorted with a
  comparator that was not a total order.
- A symlinked `notes/` or `archive/` folder was followed out of the store.
  A symlinked `.libellus` was skipped and a parent store used instead. Both
  are refused.
- A FIFO or other non-regular `ledger.meta` blocked `spend` while it held the
  ledger lock. Only a regular file is read as the cache.
- A ledger line whose `step`, `model` or `note` was not a string was accepted.
  It is now a corrupt line.
- A repeated number or boolean flag was silently ignored. Any repeated flag
  other than a list is a usage error.
- A summary shaped `[...]` broke the promoted Relatum note. It gets a
  trailing `.`.
- Lock files are created 0600, like other files. On Unix, permission denied
  on a lock is reported at once instead of waiting eight seconds and naming
  another process.

## 1.2.0

Same store for notes and ledger lines written by 1.0.0 and 1.1.2. New notes
add `seq`. A 1.1.2 binary rejects that key. New brief fields and the
`supersede` argument are additive.

- Recent decisions, questions, findings and dead ends are ordered by a
  per-agent `seq`, not by id inside one second. The brief keeps the newest
  `max_items` (default 5, at most 50). Open questions are capped too, newest
  first. `text` is at most 8192 bytes and says when it was cut.
- The brief includes the newest context note and recent findings. A decision
  can be marked replaced with `supersede`, or with an outcome that starts
  with `SUPERSEDED:` or `REVERSED:`. Replaced decisions are listed on their
  own, with `(replaced)` in the text.
- `spend` refuses before the ledger would pass 32 MiB. `brief` still returns
  the notes when the ledger cannot be read, with `ledger_error` set. `ledger`,
  `spend` and `check` still fail on a damaged or over-cap ledger.
  `libellus archive` moves the file into `archive/` and starts a fresh ledger.
  The old bytes are kept and are not part of later totals.
- Task totals are grouped in one pass. The brief's `by_task` keeps the newest
  tasks, not an alphabetical prefix, and says how many were left out.
  `libellus_ledger` still returns every task.
- A lock with no finished pid line is stale after one second. `check` removes
  stale locks and temp files whose writer is gone. A failed `spend` write
  truncates its own partial line.
- `spend` keeps a totals cache in `ledger.meta` and skips parsing when the
  file length and the last line still match. Per-task totals are cached for
  up to 64 tasks. With more tasks, spend still reads the ledger to total
  that one task. The ledger file remains the record. A same-length edit that
  leaves the last line unchanged is picked up by `brief`, `ledger` and
  `check`, which rewrite the cache. Numbers are in `docs/perf/1.2.0.md`.

## 1.1.2

Same on-disk format for a valid store. A note or ledger line that was already
invalid is no longer treated as good data.

- A fifo or other non-regular file in the notes folder is skipped, with a
  warning. `check` reports it. It is not opened. A symlink `ledger.jsonl`, and
  any other symlink store file, is refused on read and on write.
- `spend` reads the ledger once under its lock. On this machine, with the
  `examples/perf.rs` harness, a spend against 10000 stored entries was median
  14969 us and p95 15455 us. The 1.0.0 baseline on the same harness was 20240
  us and 21385 us. See `docs/perf/1.1.2.md`.
- A note file whose name is not one or two slugs is skipped by `ls` and
  `brief` and failed by `check`. `created` must be `YYYY-MM-DDTHH:MM:SSZ`.
- A ledger line with the wrong agent, a non-slug task or step, or a bad
  timestamp is left out of the totals and named by `check`. A torn last line
  is not counted. `spend` will not append until that line is finished or
  deleted. A ledger over 32 MiB, or a line over 64 KiB, is an error.
- An id made from the summary is retried when the file already exists.
  `--flag=value` works, and `--flag -- --value` passes a value that starts
  with `--`. `limit` errors say the range starts at 1. On Unix, new
  `.libellus` and `agents` directories are mode 0700.
- The body argument is at most 20000 bytes. The brief text does not always
  end with a Next line. `libellus_check` is in the core group. `check` reports
  a stale lock and says to remove that file if nothing is using it. A summary
  cannot contain a control character. Duplicate tags are stored once. An MCP
  message must say `"jsonrpc":"2.0"`. `LIBELLUS_DIR` must already be an
  initialised store.

## 1.1.1

No behaviour changes. Same on-disk format as 1.0.0.

- The Windows release archive is now a real zip. The release job checks that
  the archive starts with the PK signature and lists its entries before upload.

## 1.1.0

Same on-disk format as 1.0.0.

- `libellus_ls` takes `offset` (default 0) and `before_id`. The reply adds
  `offset`, `returned`, `truncated`, `next_offset`, and `next_before_id`.
  `before_id` is the stable cursor. `limit` must be at least 1. `total` is
  still the full count. `offset` 0 is the 1.0.0 page.
- The brief text adds a `Next:` line only when a question is open or a
  decision or dead end is not promoted. The headings and cost lines are
  unchanged. `text` stays the short form; the arrays stay for callers that
  parse them.
- A missing note, a duplicate id, and a second resolve name the call that
  fixes it. A missing `agent` mentions `LIBELLUS_AGENT`.
- A spend checks the 64-bit total before appending and does not write when it
  would overflow. `ledger` and `brief` report an already unsummable ledger as
  `ledger_error` instead of failing the whole call. `libellus_check` fails on
  it. Resolve and spend take an exclusive lock file.
- A note file over 1 MiB is an error. A note whose `agent` field does not
  match its folder is skipped by `ls` and `brief` with a warning, refused by
  `get`, and failed by `check`.
- Unknown CLI flags are reported as `unknown flag`. `libellus_check` is an
  MCP tool. `--host` on `brief` is CLI only.
- Listing and the brief read many notes at once on more than one CPU. Numbers
  are in `docs/perf/`. A spend still read the ledger twice; 1.1.2 reads it once.

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
