# Extending summonr

Optional capabilities are compiled modules. `summonr modules` lists them
with their state. A project may turn a module off. A project can't turn a
module on.

You extend summonr outside the process, with files and child processes.
This keeps the 18 MB RSS of a `-p` print run and the short first-round
prefix. There is no in-process package loader and no marketplace.

After the first model round, the advertised tool list and the skill
catalog stay frozen until the process exits. `/modules` still writes the
config for the next process.

## Add a skill

Write a `SKILL.md` under `~/.summonr/skills/<name>/` or
`.summonr/skills/<name>/`. The model loads it with the `skill` tool. The
body arrives as a later tool result.

## Add an MCP server

1. Run `/modules mcp on`.
2. Add a `[mcp.NAME]` table to `~/.summonr/config.toml`.
3. Restart summonr so the new tool aliases join the next prefix.

A project `.mcp.json` waits for your approval. Claude Code and Codex MCP
configs load only when `import_mcp = true` is set in
`~/.summonr/config.toml`. The model sees each tool as a `server__tool`
alias.

## Add a hook

Set `pre_tool`, `post_tool`, `compact`, or `stop` under `[hooks]` in
`~/.summonr/config.toml`. Each value is a shell command. summonr writes
JSON to its stdin. A `pre_tool` hook that exits nonzero blocks the call.

## Add a CLI

Put the binary on `PATH` or under `~/.summonr/bin/<name>/`. The model
runs it with `run_command`. Policy, the danger prompt, and `--sandbox`
still apply. summonr doesn't put a workspace `.summonr/bin` on `PATH`
until you approve it, the same way as `.mcp.json`.

## Install a directory of extras

`summonr add PATH` copies `skills/` and `bin/` (to
`~/.summonr/bin/<bundle>/`), and merges `mcp.toml` and `hooks.toml` into
`~/.summonr/config.toml`. If a name conflicts, summonr refuses the copy.
Copying doesn't execute a binary. `summonr add --list` prints what you
installed.

## Keep memory across sessions

Memory is a skill that reads labeled notes. It isn't a built-in store.

1. Copy `skills/memory/SKILL.md` from this repository to
   `~/.summonr/skills/memory/SKILL.md`.
2. Keep the notes in `notes/` beside that file.

## Related

- The [reference](reference.md#extend) has the table of what the model
  sees for each kind of extra.
- The [module API](internals/module-api.md) states the rule for a new
  compiled module.
