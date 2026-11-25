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

Choose tool groups with a line in the `agent-tools` block of AGENTS.md:

````markdown
```agent-tools
libellus: disable ledger
```
````
