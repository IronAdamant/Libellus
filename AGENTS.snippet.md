## Libellus: your private notepad and number ledger

This project uses Libellus (`.libellus/`). Notes are private to your agent name
(`main`, `sub-2`). Share with the swarm by writing a Relatum note.

- Write a note when you decide something, rule something out, or are unsure:
  `libellus_note` with `kind` decision, dead-end, question, finding or context.
  Keep the summary to one line.
- Point at proof instead of copying it: `evidence: ["trammel:plan-3/step-2"]`.
- Record spend per step with `libellus_spend` (numbers you report; Libellus only
  adds them).
- After your context is compacted, or when you resume, call `libellus_brief` with
  your agent name before doing anything else.
- At session start and after compaction, the host can run the brief for you.
  `libellus hook --agent main --host generic` prints the command. `--host claude`
  and `--host cursor` print a snippet to paste into that host's hook file.
  Cursor's preCompact hook cannot put text back into the model. Its
  sessionStart hook can, when a chat starts. After compaction in Cursor, call
  `libellus_brief` and put the text into the session.
- `LIBELLUS_AGENT` is the default agent name when a command needs one and you
  omit `--agent`. The ledger does not use it: with no agent it sums every agent.

Choose tool groups with a line in the `agent-tools` block of AGENTS.md:

````markdown
```agent-tools
libellus: disable ledger
```
````
