## Libellus: your private notepad and number ledger

This project uses Libellus (`.libellus/`). Notes are private to your agent name
(`main`, `sub-2`). Share one with the swarm only by `libellus_promote`, which
writes a Relatum note. Libellus does not call a model.

- Write a note when you decide something, rule something out, or are unsure:
  `libellus_note` with `kind` decision, dead-end, question, finding or context.
  Keep the summary to one line. Read it back with `libellus_get`.
- Close a question with `libellus_resolve` and a one-line outcome. A note that
  is already resolved stays as it is.
- Batch: `libellus_note` takes `notes` (up to 50) and `libellus_resolve`
  takes `items`. One call for several costs one turn.
- Point at proof instead of copying it: `evidence: ["trammel:plan-3/step-2"]`.
- The board is shared by every agent: the main agent plans a project with
  `libellus_plan` (an `outline` makes a whole tree in one call) and sets
  `owner` on the subtrees it hands out. Each agent ticks off its own tasks
  with `libellus_plan` `set` ops, several per call. A parent closes by
  itself when its children are done; a closed project is archived. Read it
  with `libellus_board`; your own open tasks are in the brief. People read
  `.libellus/board.html` and ask you for changes.
- A sub-agent uses the Libellus agent name its prompt gives it (or the one
  the SubagentStart hook gives it) and calls `libellus_brief` with it first.
- Record spend per step with `libellus_spend` (numbers you report; Libellus only
  adds them). Each call appends a line. Read totals with `libellus_ledger`.
- `libellus_ls` returns the newest page (`limit`, default 50, minimum 1).
  Pass `before_id` from `next_before_id` for the next older page. `offset` is
  the older knob and can repeat a note if one is written between calls.
  `total` is every matching note, not the page.
- At session start and after compaction, put the brief back into the session
  before doing anything else. `libellus hook --agent main --host generic` prints
  the command `libellus brief --agent main`. `--host claude` and `--host cursor`
  print a snippet to paste into that host's hook file. Cursor's preCompact hook
  cannot put text back into the model. Its sessionStart hook can, when a chat
  starts. After compaction in Cursor, call `libellus_brief` and put the text
  into the session.
- `LIBELLUS_AGENT` is the default agent name when a command needs one and you
  omit `--agent`. The ledger does not use it: with no agent it sums every agent.

Choose tool groups with a line in the `agent-tools` block of AGENTS.md, or with
`LIBELLUS_TOOLS` and `LIBELLUS_DISABLE` (those replace the file line when set).
`core` cannot be disabled. A typo falls back to the core tools and says why.

````markdown
```agent-tools
libellus: disable ledger
```
````

Groups: `core` (always on), `ledger`, `board`.
