# Changelog

## Unreleased

- The brief said "1 ledger entries"; it now says "1 ledger entry".

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
