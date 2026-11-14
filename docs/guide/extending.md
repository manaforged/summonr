# Extending summonr

Optional capabilities are compiled modules. `summonr module list` lists them with their state. A project may turn a module off. A project can't turn a module on.

You extend summonr outside the process, with files and child processes. This keeps summonr's memory small (27 MB in our benchmark runs) and its first-round prefix short. There is no in-process package loader and no marketplace. An extension is a plugin: a directory of skills, commands, agents, hooks, and MCP servers. Pi and oh-my-pi put that in one TypeScript file that runs inside the process. summonr splits the same jobs across files and does not run them inside the process.

| A Pi extension does this | An extension ships this |
| --- | --- |
| Slash command, skill, sub-agent | `commands/`, `skills/`, `agents/` |
| A tool the model calls | An MCP server or `bin/` |
| Block a tool call | A hook |
| Run code on every turn | Nothing. The harness owns that. |

A workflow a person adds is an agent plus a command, not a `[workflows]` seat map. `workflows/` in a plugin is not loaded. Copy `plugins/hillclimb` for a new one.

After the first model round, the advertised tool list and the skill catalog stay frozen until the process exits. A module switch in `/settings` still writes the config for the next process.

## Add a skill

Write a `SKILL.md` under `~/.summonr/skills/<name>/` or `.summonr/skills/<name>/`. Claude Code skills work as they are: summonr also reads `~/.claude/skills/`, `.claude/skills/` in the workspace, and `~/.agents/skills/`. The model loads it with the `skill` tool. The body arrives as a later tool result.

In Summonr's own two skill roots, skills can sit in folders up to four levels deep, such as `review/rust/SKILL.md`. A skill keeps its own directory name, and two skills with one name in different folders list as `FOLDER:NAME`. The Claude Code and `~/.agents/skills/` roots are read one level deep, as Claude Code reads them. When two roots hold a skill with the same name, the first root in the list above wins.

A skill runs when the model loads it or when you type `/NAME args`. summonr reads these frontmatter fields:

| Field | Effect |
| --- | --- |
| `name` | The name you type. The directory name also runs the skill. |
| `description`, `when_to_use` | Joined into the catalog summary, cut at 1,536 characters. With neither, the summary is the first line of the body. |
| `disable-model-invocation: true` | The skill is left out of the model's catalog, and the `skill` tool refuses it. `/NAME` still runs it. |
| `user-invocable: false` | The skill is left out of the `/` menu, and `/NAME` does not run it. The model can still load it. |
| `argument-hint` | Shown after the summary in completion. |
| `arguments` | Names for `$name` placeholders, in argument order. |
| `allowed-tools`, `model` | Read, but not applied yet. |

Booleans take `true`, `false`, `yes`, `no`, `on`, `off`, `1`, or `0`. When you type `/NAME args`, the body after the frontmatter is the prompt. Skills and commands fill these placeholders the same way:

- `$ARGUMENTS` is everything after the name, as typed.
- `$ARGUMENTS[N]` and `$N` are word `N`, counted from 0, so `$0` is the first word. Quote a value to keep its spaces. A placeholder with no word stays as typed.
- `$name` is the word at the position of `name` in `arguments`, or empty.
- A backslash before one of these keeps it as text: `\$1.00`.
- When no placeholder takes the arguments, they are added at the end as `ARGUMENTS: <args>`.
- `${CLAUDE_SKILL_DIR}` is the skill's directory and `${CLAUDE_PROJECT_DIR}` is the workspace. These are also filled when the model loads the skill.

Argument text goes in once and is not read again for `$` placeholders. `/NAME` with no arguments takes the session's current task as its arguments. When a skill and a command have the same name, the skill runs. summonr does not run `` !`cmd` `` or ` ```! ` blocks, does not attach `@file` references, and does not fill `${CLAUDE_SESSION_ID}`. They reach the model as text.

A command is a Markdown file under `commands/` in `.summonr/`, `.claude/`, `~/.summonr/`, or `~/.claude/`. A file in a subfolder is named by its path, with `:` between the parts: `.claude/commands/frontend/component.md` runs as `/frontend:component`. Commands take the same frontmatter as skills, except `name`.

## Add an MCP server

1. Run `summonr mcp add NAME -- COMMAND [ARGS...]` for a local server, or `summonr mcp add NAME --url URL` for a remote one. This writes a `[mcp.NAME]` table to `~/.summonr/config.toml`; you can also write it yourself. `mcp add` connects once, caches the tool list, and prints `NAME: N tools`. When the server does not start or answer, `mcp add` exits 1 and names the config file; the table stays, and `summonr mcp list` shows `failed: <error>`.
2. Restart summonr so the new tool aliases join the next prefix.

`summonr mcp list` shows each server's state, tool count, and how many bytes of tool schemas it adds to every request. `/mcp` shows the same in the terminal UI, with the live state of the current session. `summonr mcp remove NAME` deletes the table.

The `mcp` module is on by default. summonr lists each server's tools once and keeps the list in `~/.summonr/mcp/`. A server starts on its first tool call and stops when it is idle, so a configured server costs no memory until the model uses it. Optional keys on `[mcp.NAME]`:

- `tools = ["name", ...]` advertises only these tools.
- `idle_secs = N` stops the server after N idle seconds. The default is 120.
- `keep_alive = true` keeps the server running once it starts.
- `timeout_secs = N` cancels a call with no reply after N seconds. The default is 300. Starting a server waits at most 60 seconds, or `timeout_secs` when that is lower.

`idle_secs` and `timeout_secs` must be at least 1. These keys also work on a table that only names a server from `.mcp.json` or an imported config, so you can narrow a server you did not write; a `.mcp.json` server still needs `enabled = true` to run. `policy.toml` deny lists match tool aliases exactly or by a trailing `*`, for example `github__*`, and a denied tool is not advertised. Under `--strict`, a tool that is not marked read-only needs an allow entry. Images from a tool reach the model only when it takes images and the data is PNG, JPEG, GIF, or WebP.

summonr speaks MCP 2025-11-25 and accepts servers on 2025-06-18, 2025-03-26, and 2024-11-05. A local server can ask for the workspace root. Progress from a tool shows in the terminal UI and in `--json`, and it keeps a slow call alive for up to four times `timeout_secs`. A server that changes its tool list gets it fetched again; the new list applies from the next session. Server log messages go to `~/.summonr/logs/mcp.log`. Stopping a turn cancels the MCP call it was waiting on.

A project `.mcp.json` server runs only after you set `enabled = true` under its `[mcp.NAME]` table. summonr names the servers that wait. Claude Code and Codex MCP configs load only when `import_mcp = true` is set in `~/.summonr/config.toml`. The model sees each tool as a `server__tool` alias. A server with more than 8 KB of tool schemas is listed by tool name under one `mcp` tool instead: the model looks up schemas with `query` and calls a tool with `tool` and its arguments: an `arguments` object, an `arguments` JSON string, or fields beside `tool`. Policy and hooks see the tool behind `mcp`.

Type `/server:prompt` followed by its arguments to run a server prompt; the arguments fill the prompt's declared arguments in order, and the last one takes the rest of the line. `@server:uri` attaches a server resource to your prompt the way `@path` attaches a file. The composer suggests both from the cached lists. Both work with `-p`.

A server can ask you to fill a form during a tool call. summonr shows which server asks, takes one field at a time, checks each answer against the server's schema, and lets you review before `y` sends, `n` declines, or `esc` cancels. With no one at the keyboard (`-p`, ACP) the form is declined. Form requests work over stdio; remote servers do not get the form capability.

## Add a hook

Set an event under `[hooks]` in `~/.summonr/config.toml`: `session_start`, `user_prompt`, `pre_tool`, `post_tool`, `post_tool_failure`, `pre_compact`, `compact`, `stop`, or `session_end`. Each value is a shell command, a table `{ command = "...", dialect = "claude", matcher = "Edit|Write", timeout = 30 }`, or a list of either; the list runs in order. summonr writes JSON to each command's stdin. A `pre_tool` hook that exits nonzero blocks the call, and the rest of the list does not run. A `user_prompt` hook can block the prompt, a `stop` hook that exits 2 keeps the turn going, and plain stdout from `session_start` or `user_prompt` goes to the model as context. With `dialect = "claude"`, a hook gets Claude Code's stdin JSON and verdict rules. `matcher` takes tool names separated by `|`, or a regex, as in a Claude Code `hooks.json`, and is checked against the tool name in the hook's dialect. The [reference](reference.md#hooks) lists the stdin fields, verdicts, JSON output fields, and environment variables.

## Add a CLI

Put the binary on `PATH` or under `~/.summonr/bin/<name>/`. The model runs it with `run_command`. Policy and the danger prompt still apply. summonr doesn't put a workspace `.summonr/bin` on `PATH` until you approve it, the same way as `.mcp.json`.

## Add a panel

A panel is a tab in the desktop app's right panel, or a section of its sidebar, drawn with the app's own widgets. Write `panels/NAME.toml` under `.summonr` in the workspace, or under `~/.summonr` for every workspace. A plugin can ship the same files in its `panels` directory. `summonr panel --help` has the file format and every view: chart, stat, table, text and list.

Set `place = "side"` to put the panel in the sidebar. An icon rail then shows beside the sidebar: its first button shows the repositories and agents, and each side panel gets a button that shows that panel instead. `icon` names the button's [Lucide](https://lucide.dev/icons/) icon, such as `"inbox"` or `"git-pull-request"`; a name the app does not bundle shows a default icon. A `list` view gives each row an action: `start` opens a new agent with the row's prompt, and `open` shows an existing one. Commit the workspace's panel file to share a team's process, such as the issues assigned to you:

```toml
title = "Assigned to me"
place = "side"
icon = "inbox"
every_secs = 120
command = ["sh", "-c", "gh issue list --assignee @me --json number,title | jq '{view: \"list\", rows: map({title, meta: \"#\\(.number)\", action: {start: {prompt: \"Fix issue #\\(.number): \\(.title)\"}}})}'"]
```

A command can print an [A2UI 1.0](https://a2ui.org/specification/v1.0-a2ui/) surface instead of a view: a `createSurface` message with the basic catalog, then `updateComponents` with a component whose id is `root` and `updateDataModel`, as an array or `{"messages": [...]}`. The desktop draws its basic-catalog components (Text, Button, Row, Column, Card, List, Tabs, Image, Icon, Divider and the inputs), and inputs bind to the data model by JSON Pointer. A Button's `action.event` named `start`, with a `prompt` in its context, opens a new agent with that prompt. One named `open`, with a `session`, shows that agent. Any other event runs the command again with the A2UI `action` message on stdin (`version`, then `action` with `name`, `surfaceId` set to the panel's name, `timestamp` and `context`), and what it prints replaces the surface:

```json
[{"version":"1.0","createSurface":{"surfaceId":"queue","catalogId":"https://a2ui.org/specification/v1_0/catalogs/basic/catalog.json"}},{"version":"1.0","updateComponents":{"surfaceId":"queue","components":[
  {"id":"root","component":"List","children":{"componentId":"pr","path":"/prs"}},
  {"id":"pr","component":"Button","child":"title","action":{"event":{"name":"start","context":{"prompt":{"@path":"prompt"}}}}},
  {"id":"title","component":"Text","text":{"@path":"title"}}]}},
 {"version":"1.0","updateDataModel":{"surfaceId":"queue","path":"/","value":{"prs":[{"title":"Fix cart rounding","prompt":"Review PR #12"}]}}}]
```

A surface can also use Sigil's own components beside the basic catalog: `Chart`, `Stat`, `Table`, `Markdown`, `Mermaid`, `Diff`, and `Item`, a clickable row with a `title`, `status`, `detail`, `marker`, `tone`, `depth` and `action`. Their schemas are Sigil's A2UI catalog.

With `kind = "stream"`, the command keeps running while the panel shows and stops when it closes. Each line it prints is a whole surface that replaces the last one, and each button event reaches its stdin as one A2UI action line, so a script can push updates as they happen instead of polling with `every_secs`.

The command runs only while its tab is open, in the workspace, with the same environment as a hook. With `--no-trust`, workspace panels are ignored. A project's panel runs a command from that repository when you open its tab, so treat the tab like running a script from the repository.

## Install a plugin

A plugin is a directory in the Claude Code plugin layout. `summonr plugin install PATH` copies it to `~/.summonr/plugins/<name>/`; the name comes from `.claude-plugin/plugin.json`, or from the directory name. Copying does not run anything inside summonr. After the copy, install connects to each MCP server the plugin adds. Restart summonr to load the plugin.

| Part | What summonr does with it |
| --- | --- |
| `skills/<name>/SKILL.md`, manifest `skills` | Skills named `/<plugin>:<name>`, or `/<name>` when no other skill or command has the name. A skill of your own with the same name does not hide the plugin skill. Manifest `skills` adds folders to `skills/`; `"."` names the plugin root. A plugin with only a root `SKILL.md` is one skill named after the plugin. `${CLAUDE_PLUGIN_ROOT}` and `${CLAUDE_PLUGIN_DATA}` in the body are filled. |
| `.mcp.json`, manifest `mcpServers`, `mcp.toml` | MCP servers named `plugin_<plugin>_<server>`, so a tool is `mcp__plugin_<plugin>_<server>__<tool>` in hooks, as in Claude Code. Manifest `mcpServers` takes a `.json` path, an inline map, or an array of both; a later server of the same name replaces an earlier one. `.mcpb` and `.dxt` bundles are skipped. Installing counts as approval. |
| `hooks/hooks.json`, manifest `hooks`, `hooks.toml` | Hooks added after your own. Manifest `hooks` takes a `.json` path, an inline event map, or an array of both. |
| `bin/` | On `PATH` for `run_command` |
| `commands/<name>.md`, manifest `commands` | Slash commands: `/<plugin>:<name> args`, or `/<name> args` when no other plugin or built-in command has the name. Placeholders are filled as for [skills](#add-a-skill). Manifest `commands` takes a path, a list of paths to files or folders, or a map from name to `source` or `content` with `description` and `argumentHint`. When it is set, `commands/` is not read. They show in completion. |
| `agents/<name>.md`, manifest `agents` | Agents the model can hand a `task` to by name. Manifest `agents` lists `.md` files and replaces `agents/`. A file in `agents/review/` is `<plugin>:review:<name>`. The frontmatter `description` is listed to the model; the body opens the child's first message; `tools` (Claude or summonr names) limits what the child may call, less any `disallowedTools`. `model` and `color` are read and not applied, so the child keeps the parent's prompt cache; `install` names them as skipped. |
| `.lsp.json`, manifest `lspServers` | Language servers for the listed extensions, started on first use like built-in ones. Manifest `lspServers` takes a `.json` path, an inline map, or an array of both. summonr applies `command`, `args`, `env`, `extensionToLanguage`, `initializationOptions`, and `workspaceFolder`, and runs every server over stdio; `install` names each other field as skipped. Your own `[lsp]` settings come first. |

`${CLAUDE_PLUGIN_ROOT}` and `${SUMMONR_PLUGIN_ROOT}` in a server's command, args, env, and URL become the plugin directory, `${CLAUDE_PLUGIN_DATA}` becomes `~/.summonr/plugin-data/<name>/` (deleted on uninstall), and `${CLAUDE_PROJECT_DIR}` becomes the workspace; hook commands get the same values as environment variables. summonr does not prompt for a plugin's `userConfig`. `install` names it as skipped and names each `${user_config.KEY}`. Set the key under `[mcp.plugin_<plugin>_<server>.user_config]`, or set `env` on `[mcp.plugin_<plugin>_<server>]` without repeating `command` or `url`. A table named `[mcp.<plugin>-<server>]`, the name earlier releases used, still applies to that server. A missing key is empty. `enabled = false` on that table turns that one server off. Hooks from `hooks.json` run the way Claude Code runs them: their stdin carries `tool_name` (`Bash` for `run_command`, `Grep` for `search`, or `Glob` for a name search, `mcp__<server>__<tool>` for MCP tools), `tool_input`, and `hook_event_name`; `matcher` selects tools; exit 2, or `permissionDecision: "deny"` on stdout, blocks a `PreToolUse` call; any other nonzero exit is a note, not a block. summonr maps `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PreCompact`, `PostCompact`, `Stop`, and `SessionEnd`, and runs `command` hooks only. `monitors/`, `output-styles/`, `workflows/`, and `themes/` are not loaded; `install` names the ones it skipped.

Nothing a plugin ships runs inside summonr. Skills, commands, and agents are files. Hooks and `bin/` tools run once per call. An MCP server starts once to list its tools, then on first use; a language server starts on first use. Both stop on their own when idle. Beyond `servers_max` live servers, the least recently used idle server stops. `/mcp` shows each running server's memory on macOS and Linux.

`summonr plugin list` lists plugins and what each adds. `summonr plugin remove NAME` deletes the directory and `~/.summonr/plugin-data/<name>/`, and it removes that name from `[plugins] off`. `[plugins] off = ["NAME"]` in `~/.summonr/config.toml` keeps one installed but unloaded. A name in `off` that is not installed is an error when a session starts and when you run `summonr plugin list`.

`install` refuses a directory that adds nothing summonr loads, and it refuses `plugin.json`, `.mcp.json`, `hooks.json`, `.lsp.json`, `mcp.toml`, or `hooks.toml` when the file is present, not empty, and does not parse. The same parse check applies to a JSON file the manifest points at. It connects to each new MCP server. If a server does not answer, `install` exits 1 and keeps the copy. Installing the same name again replaces the directory and keeps `~/.summonr/plugin-data/<name>/`. A hook whose type is not `command` is skipped and does not run. A manifest path may start with `./` or not; one that leaves the plugin directory or does not exist is skipped and named.

Agents also load from `.claude/agents/` in the workspace and `~/.claude/agents/`, subfolders included, named by the frontmatter `name`. On a clash the workspace agent wins over the user agent, and both win over a plugin agent of the same bare name.

## Related

- The [reference](reference.md#extend) has the table of what the model sees for each kind of extra.
- The [module API](internals/module-api.md) states the rule for a new compiled module.
