# memory

Use when a fact, decision, or episode should survive this session.

Directory: `~/.summonr/skills/memory/notes/` (create it). One file per note. Name files with a label and date: `decision-2026-09-19.md`, `fact-….md`, `episode-….md`.

On the first turn of a related task, `read` that directory (or `run_command` `ls`) and open the notes that match. Before you stop, `write` or `edit` a note if you learned something the next session will need.

Do not put notes in the system prompt. Do not invent a second store. If the directory is empty, say so and continue.
