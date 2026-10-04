# Summonr

Build, check, and test commands are in [CONTRIBUTING.md](CONTRIBUTING.md).

## Architecture invariants

- Shared API: `docs/guide/internals/module-api.md`. See [API](#api).
- Goal: the leanest, easiest-to-use open-source agent harness. Line growth, command sprawl, and first-run friction are defects.
- Summonr never refuses to start or edit. No live-checkout guard, no process-list gate, no escape-hatch variable.
- No cap on provider rounds, tool calls, or reasoning. Only a token or wall-clock budget that the user sets ends a turn.
- Never print an API key or token fragment: no prefix, suffix, or masked form. Name a secret by location and type only.
- UI comes from Sigil. Fix a UI defect in Sigil, not here.

## API

Locked. `docs/guide/internals/module-api.md` is the menu: one name per job, and a
"Names we delete" table. A new shared helper is a row on that page in
the same change, or it stays private. `python3 scripts/api-census.py
--check` must print `api-census deny: clean`.

Official extras are compiled built-in modules. User extras are
skills, MCP servers, hooks, and CLIs via `run_command`. Do not add an
in-process plugin host, `dlopen`, Wasmtime, or a marketplace. Do not
add a model-facing tool when `skill` or `run_command` already does the
job. A new compiled module needs a `module-api.md` row and an RSS /
prefix review.
