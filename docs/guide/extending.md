# Extending summonr

Optional capabilities are compiled modules. `summonr modules` lists them
with their state. A project may turn a module off. A project can't turn a
module on.

You extend summonr outside the process, with files and child processes.
This keeps the 18 MB RSS of a `-p` print run and the short first-round
prefix. There is no in-process package loader and no marketplace.

After the first model round, the advertised tool list and the skill
catalog stay frozen until the process exits. A module switch in
`/settings` still writes the config for the next process.

## Add a skill

Write a `SKILL.md` under `~/.summonr/skills/<name>/` or
`.summonr/skills/<name>/`. The model loads it with the `skill` tool. The
body arrives as a later tool result.

## Add an MCP server

1. Run `summonr mcp add NAME -- COMMAND [ARGS...]` for a local server, or
   `summonr mcp add NAME --url URL` for a remote one. This writes a
   `[mcp.NAME]` table to `~/.summonr/config.toml`; you can also write it
   yourself.
2. Restart summonr so the new tool aliases join the next prefix.

`summonr mcp list` shows each server's state, tool count, and how many
bytes of tool schemas it adds to every request. `/mcp` shows the same in
the terminal UI, with the live state of the current session.
`summonr mcp remove NAME` deletes the table.

The `mcp` module is on by default. Summonr lists each server's tools once
and keeps the list in `~/.summonr/mcp/`. A server starts on its first tool
call and stops when it is idle, so a configured server costs no memory
until the model uses it. Optional keys on `[mcp.NAME]`:

- `tools = ["name", ...]` advertises only these tools.
- `idle_secs = N` stops the server after N idle seconds. The default is 120.
- `keep_alive = true` keeps the server running once it starts.
- `timeout_secs = N` cancels a call with no reply after N seconds. The
  default is 300. Starting a server waits at most 60 seconds, or
  `timeout_secs` when that is lower.

`idle_secs` and `timeout_secs` must be at least 1. These keys also work
on a table that only names a server from `.mcp.json` or an imported
config, so you can narrow a server you did not write; a `.mcp.json`
server still needs `enabled = true` to run. `policy.toml` deny lists
match tool aliases exactly or by a trailing `*`, for example `github__*`,
and a denied tool is not advertised. Under `--strict`, a tool that is not
marked read-only needs an allow entry. Under `--sandbox`, stdio servers
run in the sandbox, and a server restarts when the sandbox changes.
Images from a tool reach the model only when it takes images and the
data is PNG, JPEG, GIF, or WebP.

Summonr speaks MCP 2025-11-25 and accepts servers on 2025-06-18,
2025-03-26, and 2024-11-05. A local server can ask for the workspace
root. Progress from a tool shows in the terminal UI and in `--json`, and
it keeps a slow call alive for up to four times `timeout_secs`. A server
that changes its tool list gets it fetched again; the new list applies
from the next session. Server log messages go to
`~/.summonr/logs/mcp.log`. Stopping a turn cancels the MCP call it was
waiting on.

A project `.mcp.json` server runs only after you set `enabled = true`
under its `[mcp.NAME]` table. Summonr names the servers that wait. Claude Code and Codex MCP
configs load only when `import_mcp = true` is set in
`~/.summonr/config.toml`. The model sees each tool as a `server__tool`
alias. A server with more than 8 KB of tool schemas is listed by tool
name under one `mcp` tool instead: the model looks up schemas with
`query` and calls a tool with `tool` and `arguments`. Policy and hooks
see the tool behind `mcp`.

Type `/server:prompt` followed by its arguments to run a server prompt;
the arguments fill the prompt's declared arguments in order, and the
last one takes the rest of the line. `@server:uri` attaches a server
resource to your prompt the way `@path` attaches a file. The composer
suggests both from the cached lists. Both work with `-p`.

A server can ask you to fill a form during a tool call. Summonr shows
which server asks, takes one field at a time, checks each answer against
the server's schema, and lets you review before `y` sends, `n` declines,
or `esc` cancels. With no one at the keyboard (`-p`, ACP) the form is
declined. Form requests work over stdio; remote servers do not get the
form capability.

## Add a hook

Set `pre_tool`, `post_tool`, `compact`, or `stop` under `[hooks]` in
`~/.summonr/config.toml`. Each value is a shell command or a list of
commands; the list runs in order. summonr writes JSON to each command's
stdin. A `pre_tool` hook that exits nonzero blocks the call, and the
rest of the list does not run.

## Add a CLI

Put the binary on `PATH` or under `~/.summonr/bin/<name>/`. The model
runs it with `run_command`. Policy, the danger prompt, and `--sandbox`
still apply. summonr doesn't put a workspace `.summonr/bin` on `PATH`
until you approve it, the same way as `.mcp.json`.

## Install a plugin

A plugin is a directory in the Claude Code plugin layout.
`summonr install PATH` copies it to `~/.summonr/plugins/<name>/`; the
name comes from `.claude-plugin/plugin.json`, or from the directory
name. Copying does not run anything. Restart summonr to load it.

| Part | What summonr does with it |
| --- | --- |
| `skills/<name>/SKILL.md`, manifest `skills` | Added to the skill catalog; the summary is the frontmatter `description` |
| `.mcp.json`, manifest `mcpServers`, `mcp.toml` | MCP servers named `<plugin>-<server>`; installing counts as approval |
| `hooks/hooks.json`, manifest `hooks`, `hooks.toml` | Hooks added after your own |
| `bin/` | On `PATH` for `run_command` |

`${CLAUDE_PLUGIN_ROOT}` and `${SUMMONR_PLUGIN_ROOT}` in a server's
command, args, env, and URL become the plugin directory,
`${CLAUDE_PLUGIN_DATA}` becomes `~/.summonr/plugin-data/<name>/` (deleted
on uninstall), and `${CLAUDE_PROJECT_DIR}` becomes the workspace; hook
commands get the same values as environment variables. summonr does not
prompt for a plugin's `userConfig`: `install` names it as skipped and
warns about each server that uses `${user_config.KEY}`, which stays
empty until you set the value in that server's `[mcp.<plugin>-<server>]`
table. Hooks from `hooks.json` run the way
Claude Code runs them: their stdin carries `tool_name` (`Bash` for
`run_command`, `mcp__<server>__<tool>` for MCP tools), `tool_input`, and
`hook_event_name`; `matcher` selects tools; exit 2, or
`permissionDecision: "deny"` on stdout, blocks a `PreToolUse` call; any
other nonzero exit is a note, not a block. summonr maps `PreToolUse`,
`PostToolUse`, `PreCompact`, and `Stop`, and runs `command` hooks only.
`commands/`, `agents/`, `monitors/`, `output-styles/`, `workflows/`,
`themes/`, and `.lsp.json` are not loaded; `install` names the ones it
skipped.

`summonr install --list` lists plugins and what each adds.
`summonr uninstall NAME` deletes one. `[plugins] off = ["NAME"]` in
`~/.summonr/config.toml` keeps one installed but unloaded.

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
