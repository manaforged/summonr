# Extending summonr

Optional capabilities are compiled modules. `summonr module list` lists them with their state. A project may turn a module off. A project can't turn a module on.

You extend summonr outside the process, with files and child processes. This keeps summonr's memory small (27 MB in our benchmark runs) and its first-round prefix short. There is no in-process package loader. An extension is a plugin: a directory of skills, commands, agents, hooks, and MCP servers. Pi and oh-my-pi put that in one TypeScript file that runs inside the process. summonr splits the same jobs across files and does not run them inside the process.

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
| `allowed-tools`, `disallowed-tools`, `model` other than `inherit`, `effort`, `context`, `agent`, `background`, `hooks`, `paths`, `shell` | Not applied. When a session starts, summonr names each of these that your own or the workspace's skills and commands set, once per process. `summonr plugin install` lists each one a plugin's skills and commands set as `skipped`. |

Booleans take `true`, `false`, `yes`, `no`, `on`, `off`, `1`, or `0`. The body after the frontmatter is the prompt when you type `/NAME args` and the tool result when the model loads the skill; the frontmatter is in neither. Skills and commands fill these placeholders the same way:

- `$ARGUMENTS` is everything after the name, as typed.
- `$ARGUMENTS[N]` and `$N` are word `N`, counted from 0, so `$0` is the first word. Quote a value to keep its spaces. A placeholder with no word stays as typed.
- `$name` is the word at the position of `name` in `arguments`, or empty.
- A backslash before one of these keeps it as text: `\$1.00`.
- When no placeholder takes the arguments, they are added at the end as `ARGUMENTS: <args>`.
- `${CLAUDE_SKILL_DIR}` is the skill's directory, `${CLAUDE_PROJECT_DIR}` is the workspace, and `${CLAUDE_SESSION_ID}` is the session id. These are also filled when the model loads the skill.

Argument text goes in once and is not read again for `$` placeholders. `/NAME` with no arguments takes the session's current task as its arguments. When a skill and a command have the same name, the skill runs.

A body can pull in live context before the model sees it, when you type `/NAME` and when the model loads the skill:

- `` !`cmd` `` at the start of a line or after a space runs `cmd` and puts its output in place of the block. After any other character, as in `` KEY=!`cmd` ``, it stays as text.
- A fenced block that opens with ` ```! ` runs its lines as one script and puts the output in place of the block.
- A block written inside Markdown code stays as text: in a code span, as in ``` `` !`cmd` `` ```, or in a fenced code block, such as a `markdown` example that shows a ` ```! ` block. A fence closes at a line of the same character, at least as long, with nothing after it, as in CommonMark.
- `@path` attaches that file after the body, the way `@path` does in a prompt. `@"path with spaces"` must name a file; a bare `@word` that names no file stays as text.

summonr finds the blocks once, in the file as written. Command output and argument text are not read again for blocks, and `$ARGUMENTS`, `$N`, and `$name` are not filled inside a block; the `${CLAUDE_*}` placeholders are. Each block runs as a `run_command` call would, in the workspace, with the same policy, timeout, and output limits. The block never asks for approval: a command that the policy denies or that needs approval stops the whole invocation with `Shell command permission check failed for pattern "..."`. A command that exits non-zero stops it with `Shell command failed for pattern "..."` and the output. Exit code 1 from one `grep`, `rg`, `egrep`, `fgrep`, `find`, `diff`, `test`, `[`, `git diff`, or `git grep` counts as a result. Append `|| true` to another command that can exit non-zero. `@path` reads go through the `read` tool's rules, so a protected path such as `.env` stops the invocation. Command output and attached files are scrubbed for secrets.

Skills and commands under the workspace's `.summonr/` and `.claude/` run their blocks only in a trusted session. With `--no-trust`, each block is replaced with `[shell command execution disabled by policy]`. User and plugin skills and commands run their blocks in every session, as your user-level instruction files load in every session.

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
- `timeout = MS` sets the same limit in milliseconds, as Claude Code writes it. A value below 1000 is ignored. `timeout_secs` wins when both are set.
- `headers_helper = "CMD"` (or `headersHelper`) runs CMD in a shell each time summonr connects to a remote server. CMD must print a JSON object of string headers within 10 seconds; those headers replace static `headers` of the same name. CMD gets `CLAUDE_CODE_MCP_SERVER_NAME` and `CLAUDE_CODE_MCP_SERVER_URL`, and `CLAUDE_PLUGIN_ROOT` for a plugin server. A helper from a project `.mcp.json` or a plugin runs with a clean environment; one from your own config gets yours.

`idle_secs` and `timeout_secs` must be at least 1. These keys also work on a table that only names a server from `.mcp.json` or an imported config, so you can narrow a server you did not write; a `.mcp.json` server still needs `enabled = true` to run. `policy.toml` deny lists match tool aliases exactly or by a trailing `*`, for example `github__*`, and a denied tool is not advertised. Under `--strict`, a tool that is not marked read-only needs an allow entry. Images from a tool reach the model only when it takes images and the data is PNG, JPEG, GIF, or WebP.

summonr speaks MCP 2025-11-25 and accepts servers on 2025-06-18, 2025-03-26, and 2024-11-05. A local server can ask for the workspace root. Progress from a tool shows in the terminal UI and in `--json`, and it keeps a slow call alive for up to four times `timeout_secs`. A server that changes its tool list gets it fetched again; the new list applies from the next session. Server log messages go to `~/.summonr/logs/mcp.log`. Stopping a turn cancels the MCP call it was waiting on.

A project `.mcp.json` server runs only after you set `enabled = true` under its `[mcp.NAME]` table, in `-p` runs too: a cloned repository cannot start its own commands. summonr names the servers that wait. Claude Code and Codex MCP configs load only when `import_mcp = true` is set in `~/.summonr/config.toml`. When one name is defined in more than one place, summonr takes the whole entry from the first of: the project's entry in `~/.claude.json` (local scope), `.mcp.json` and `.summonr/config.toml` (project scope), `~/.claude.json` `mcpServers` (user scope), `~/.codex/config.toml`, then plugins; fields are not merged. A plugin server with the same URL or command line as one of those is not started a second time. Your `[mcp.NAME]` table applies last: with `command` or `url` it replaces the entry, and without them it changes only the keys it sets.

Entries take Claude Code's `type` field: `stdio`, `http`, `streamable-http`, or `sse`, which summonr connects to over streamable HTTP. A `ws` or `sdk` server is skipped, and summonr names it with the reason. `${VAR}` and `${VAR:-default}` in `command`, `args`, `env`, `url`, and `headers` take the value of VAR; an unset VAR without a default stays as written, and a VAR set to an empty string counts as set. The `oauth` object is not read; use `auth`. The model sees each tool as a `server__tool` alias. A server with more than 8 KB of tool schemas is listed by tool name under one `mcp` tool instead: the model looks up schemas with `query` and calls a tool with `tool` and its arguments: an `arguments` object, an `arguments` JSON string, or fields beside `tool`. Policy and hooks see the tool behind `mcp`.

Type `/server:prompt` or `/mcp__server__prompt` followed by its arguments to run a server prompt; in the second form, characters in the server name other than letters, digits, `_`, and `-` become `_`; the arguments fill the prompt's declared arguments in order, and the last one takes the rest of the line. `@server:uri` attaches a server resource to your prompt the way `@path` attaches a file. The composer suggests both from the cached lists. Both work with `-p`.

A server can ask you to fill a form during a tool call. summonr shows which server asks, takes one field at a time, checks each answer against the server's schema, and lets you review before `y` sends, `n` declines, or `esc` cancels. With no one at the keyboard (`-p`, ACP) the form is declined. Form requests work over stdio; remote servers do not get the form capability.

## Add a hook

Set an event under `[hooks]` in `~/.summonr/config.toml`: `session_start`, `user_prompt`, `pre_tool`, `permission_request`, `post_tool`, `post_tool_failure`, `notification`, `subagent_start`, `subagent_stop`, `pre_compact`, `compact`, `stop`, `stop_failure`, or `session_end`. Each value is a shell command, a table `{ command = "...", dialect = "claude", matcher = "Edit|Write", timeout = 30 }`, or a list of either; the list runs in order. summonr writes JSON to each command's stdin. A `pre_tool` hook that exits nonzero blocks the call, and the rest of the list does not run. A `user_prompt` hook can block the prompt, a `stop` or `subagent_stop` hook that exits 2 keeps the lead or the child going, and plain stdout from `session_start` or `user_prompt` goes to the model as context. A `permission_request` hook can answer an approval prompt before it shows. A `notification` hook runs when summonr waits for your approval or for a form. A `stop_failure` hook runs in place of `stop` when a provider error ends the turn. With `dialect = "claude"`, a hook gets Claude Code's stdin JSON and verdict rules. `matcher` takes tool names separated by `|`, or a regex, as in a Claude Code `hooks.json`, and is checked against the tool name in the hook's dialect. The [reference](reference.md#hooks) lists the stdin fields, verdicts, JSON output fields, and environment variables.

## Add an output style

An output style is a Markdown file that sets the role, tone, and response format of every reply in a session, as in [Claude Code](https://code.claude.com/docs/en/output-styles). summonr reads each `.md` file in `~/.claude/output-styles/`, in `.claude/output-styles/` in each folder from the workspace up to the repository root, and in an enabled plugin's `output-styles/` folder. Project styles load only in a trusted workspace. When two files give one name, the project file closest to the workspace wins, then the user file, then a plugin file.

The file name is the style name unless the front matter sets `name`. `description` shows in the `/output-style` list. `keep-coding-instructions: true` keeps the coding part of the built-in preamble: its first line, `Be concise in your responses`, and `Finish the whole request before you reply`. Without it, a style keeps only the tool rules. `force-for-plugin: true` in a plugin's style applies it whenever the plugin is enabled and overrides `output_style`; with several, the first plugin by name wins. A `~/.summonr/SYSTEM.md` preamble and `SUMMONR_SYSTEM_PROMPT` have no coding part to keep; the style is added after `SYSTEM.md`, and `SUMMONR_SYSTEM_PROMPT` replaces the whole prompt, styles included.

```markdown
---
name: Diagrams first
description: Lead every explanation with a diagram
keep-coding-instructions: true
---

When explaining code, start with a Mermaid diagram, then explain in prose.
```

Set `output_style = "Diagrams first"` in `~/.summonr/config.toml`, or in the workspace `.summonr/config.toml` of a trusted workspace, which wins. The value matches a style name exactly, case included; any other value, or `default`, means no style. `/output-style` lists the styles and marks the current one, and `/output-style NAME` switches this session, with case ignored. The switch is not saved.

A new session puts the style in its system prompt, before the `<cwd>` line. A switch during a session leaves the system prompt alone, so the cached prefix stays valid, and sends the new style's instructions at the top of your next message. `/clear` and compaction rewrite the history, and then the system prompt takes the new style. Child tasks run without the style.

## Import Claude Code settings

Set `import_claude_settings = true` in `~/.summonr/config.toml` to read Claude Code's settings files. summonr reads `~/.claude/settings.json` in every session, and `.claude/settings.json` and `.claude/settings.local.json` at the workspace root only when the workspace is trusted. It reads no managed settings. summonr maps four keys:

- `hooks`: each command handler on an event summonr has runs as a `dialect = "claude"` hook, after your `[hooks]` and plugin hooks.
- `permissions.deny` and `permissions.ask`: `Bash(cmd)`, `Bash(cmd *)`, and `Bash(cmd:*)` become the policy pattern `cmd`, and `mcp__server`, `mcp__server__*`, and `mcp__server__tool` become a deny on `server__*` or `server__tool`. A summonr pattern matches any command that contains it, so it blocks at least what the Claude Code rule blocks. Deny rules apply under `--strict`, the same as `policy.toml`.
- `env`: tool commands and hooks get these variables. Your endpoint's `env` wins, and a variable summonr sets itself, such as `PATH`, `HOME`, or `SUMMONR_HOME`, is skipped.
- `outputStyle`: the style a session starts with when no `output_style` key is set.

summonr never imports `permissions.allow` or `permissions.defaultMode`, because neither has a summonr rule that only narrows what runs. When a file is not valid JSON, or a rule, hook event, or handler has no summonr equivalent, summonr prints a note at session start and skips that item; the session still starts. `summonr doctor` lists the settings files it reads. The [reference](reference.md#claude-code-settings) has the precedence and the keys it does not read.

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
| `commands/<name>.md`, manifest `commands` | Slash commands: `/<plugin>:<name> args`, or `/<name> args` when no other plugin or built-in command has the name. Placeholders are filled as for [skills](#add-a-skill). Manifest `commands` takes a path, a list of paths to files or folders, or a map from name to `source` or `content` with `description` and `argumentHint`; `install` lists `model` and `allowedTools` as skipped. When it is set, `commands/` is not read. They show in completion. |
| `agents/<name>.md`, manifest `agents` | Agents the model can hand a `task` to by name. Manifest `agents` lists `.md` files and replaces `agents/`. A file in `agents/review/` is `<plugin>:review:<name>`. The frontmatter `description` is listed to the model; the body opens the child's first message; `tools` (Claude or summonr names, or `mcp__<server>` or `mcp__<server>__*` for one MCP server's tools) limits what the child may call; without it the child gets every tool. `disallowedTools` removes tools from that list, or from every tool: `mcp__<server>` removes one server's tools, `mcp__*` every MCP tool, and `Bash(git log *)` the whole `run_command` tool. Both fields take a YAML list or names separated by commas or spaces. A child whose `tools` entries are all removed by `disallowedTools` does not start. The mail tools are tools like any other: a named child with a `tools` list gets `send`, `board`, `inbox`, and `agents` only when the list names them, and `SendMessage` names all four. `skills` puts the full text of each named skill, up to `agent_skills_max`, in the child's first message, rendered as the `skill` tool renders it; a missing skill is skipped and named in the child's result. `model` takes `sonnet`, `opus`, `haiku`, `fable`, a full model ID, or `inherit`. It applies when the lead runs a Claude model on a configured endpoint that resolves the value to a Claude model; otherwise the child keeps the lead's model and its result names the ignored value. A tool list or a model gives the child a request prefix that differs from the lead's. `color` is read and not applied; `install` names it as skipped. |
| `.lsp.json`, manifest `lspServers` | Language servers for the listed extensions, started on first use like built-in ones. Manifest `lspServers` takes a `.json` path, an inline map, or an array of both. summonr applies `command`, `args`, `env`, `extensionToLanguage`, `initializationOptions`, and `workspaceFolder`, and runs every server over stdio; `install` names each other field as skipped. Your own `[lsp]` settings come first. |

`${CLAUDE_PLUGIN_ROOT}` and `${SUMMONR_PLUGIN_ROOT}` in a server's command, args, env, URL, headers, and `headersHelper` become the plugin directory, `${CLAUDE_PLUGIN_DATA}` becomes `~/.summonr/plugin-data/<name>/` (deleted on uninstall), and `${CLAUDE_PROJECT_DIR}` becomes the workspace; hook commands get the same values as environment variables. summonr does not prompt for a plugin's `userConfig`. `install` names it as skipped and names each `${user_config.KEY}`. Set the key under `[mcp.plugin_<plugin>_<server>.user_config]`, or set `env` on `[mcp.plugin_<plugin>_<server>]` without repeating `command` or `url`. A table named `[mcp.<plugin>-<server>]`, the name earlier releases used, still applies to that server. A missing key is empty. A `headersHelper` that names `${user_config.KEY}` is skipped; put the value in `headers`. `enabled = false` on that table turns that one server off. Hooks from `hooks.json` run the way Claude Code runs them: their stdin carries `tool_name` (`Bash` for `run_command`, `Grep` for `search`, or `Glob` for a name search, `mcp__<server>__<tool>` for MCP tools), `tool_input`, and `hook_event_name`; `matcher` selects tools; exit 2, or `permissionDecision: "deny"` on stdout, blocks a `PreToolUse` call; any other nonzero exit is a note, not a block. summonr maps `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `PreCompact`, `PostCompact`, `Stop`, and `SessionEnd`, and runs `command` hooks only. A plugin's `output-styles/` folder, or the files and folders its `outputStyles` key names in place of that folder, adds output styles (see [Add an output style](#add-an-output-style)). `monitors/`, `workflows/`, and `themes/` are not loaded; `install` names the ones it skipped.

Nothing a plugin ships runs inside summonr. Skills, commands, and agents are files. Hooks and `bin/` tools run once per call. An MCP server starts once to list its tools, then on first use; a language server starts on first use. Both stop on their own when idle. Beyond `servers_max` live servers, the least recently used idle server stops. `/mcp` shows each running server's memory on macOS and Linux.

`summonr plugin list` lists plugins and what each adds. `summonr plugin remove NAME` deletes the directory and `~/.summonr/plugin-data/<name>/`, and it removes that name from `[plugins] off`. `[plugins] off = ["NAME"]` in `~/.summonr/config.toml` keeps one installed but unloaded. A name in `off` that is not installed is an error when a session starts and when you run `summonr plugin list`.

`install` refuses a directory that adds nothing summonr loads, and it refuses `plugin.json`, `.mcp.json`, `hooks.json`, `.lsp.json`, `mcp.toml`, or `hooks.toml` when the file is present, not empty, and does not parse. The same parse check applies to a JSON file the manifest points at. It connects to each new MCP server. If a server does not answer, `install` exits 1 and keeps the copy. Installing the same name again replaces the directory and keeps `~/.summonr/plugin-data/<name>/`. A hook whose type is not `command` is skipped and does not run. A manifest path may start with `./` or not; one that leaves the plugin directory or does not exist is skipped and named.

Agents also load from `.claude/agents/` in the workspace and in each folder above it up to the repository root, and from `~/.claude/agents/`, subfolders included, named by the frontmatter `name`. A file there without `name` or `description` is skipped. On a clash the folder closest to the workspace wins, a workspace agent wins over a user agent, and both win over a plugin agent of the same bare name.

## Install from a marketplace

A marketplace is a directory or git repository with `.claude-plugin/marketplace.json`, in the Claude Code format. `summonr plugin marketplace add SOURCE` adds one and stores it under `~/.summonr/marketplaces/<name>/`, named by the file's own `name`. SOURCE is `owner/repo` on GitHub (`#ref` or `@ref` pins a branch or tag), a git URL (`https://...git`, a `github.com` or `gitlab.com` repository URL, or `user@host:path`; `#ref` pins), any other `https://` URL to a `marketplace.json`, or a path to a directory or a `.json` file, which is read in place. summonr clones a git source with `--depth 1` and never runs anything from it.

`summonr plugin install NAME@MARKETPLACE` finds the entry named NAME and fetches its `source`: a relative path inside the marketplace (`./plugins/x`, `.`, or a bare name under `metadata.pluginRoot`), `github` (`repo`), `url` (a git URL), or `git-subdir` (`url` and `path`, fetched with a sparse checkout). `ref` picks a branch or tag; `sha`, a full 40-character commit, wins over `ref`. Then it installs the result as `plugin install PATH` does. A relative path with `..`, or one that leaves the marketplace through a link, is refused, and so is a fetched link that names a directory or points outside the plugin. A marketplace added from a `marketplace.json` URL has no files, so its relative sources do not resolve. `npm`, `archive`, and `command` sources are refused: summonr does not run a command or download an archive at install time.

The entry and the plugin's own `plugin.json` combine as in Claude Code. Without `plugin.json`, the entry is the manifest. With it, `strict: true` (the default) appends the entry's `commands`, `agents`, `skills`, `outputStyles`, and `themes`, and the entry's `hooks` replace the manifest's inline hooks event by event; `strict: false` with any of those fields is a conflict and the install fails. The entry's display fields win, and `plugin.json`'s `version` wins.

`summonr plugin marketplace list [--json]` lists marketplaces. `summonr plugin marketplace update [NAME]` fetches one again, or all. `summonr plugin marketplace remove NAME` deletes its copy and uninstalls the plugins installed from it. Each git call and download stops after `[limits] plugin_fetch_secs`.

## Related

- The [reference](reference.md#extend) has the table of what the model sees for each kind of extra.
- The [module API](internals/module-api.md) states the rule for a new compiled module.
