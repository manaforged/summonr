# Changelog

## Unreleased

First stable release.

### Added

- Plain `grep -r`, `rg`, `find -name`, and `mdfind -name` commands, alone, joined by `;`, or piped into `head`, are answered by `search`, the way `cat FILE` is answered by `read`. The result names the search call it ran, so a model that habitually types shell search gets the fast, protected search without changing its habits.
- Name searches over large paths outside the workspace answer from a file index: the first slow search builds it in the background, and later ones over a home folder of 1.5 million files take about half a second instead of a 20-second walk. Each hit is checked on disk, the result says how old the index is, and `search_index_secs` and `search_index_after_ms` in `[limits]` tune it.
- `search` finds files by words in their names (`name: "q3 report"`), matches whole words (`word`), takes a list of patterns, keeps files changed within a span (`newer: "2d"`), counts matches per folder (`output: "folders"`), and pages with `offset`; past the last page it says so, and a result over 50 KB is cut with a note. Its schema is 15 arguments, with `output` (`lines`, `files`, `count`, `folders`) in place of separate switches; the older names still work. A glob without wildcards matches names that contain it, `no_ignore` also reaches build folders, and results name unreadable folders and paths that could not be opened. Every argument carries its own description for the model.
- Several sessions stay live in one window or terminal. `/new` starts a session beside the current one, and `/resume` or a click in the window's sidebar switches without restarting. The sidebar groups open sessions by project and marks each as working, needs you, ready, or failed. A session open in one summonr cannot be opened in another.
- The window draws its own conversation view: tool calls fold into a steps card, code blocks carry a copy button, and the composer holds the attach, model, and mode controls.
- `summonr login openai` offers Sign in with ChatGPT (Plus or Pro) next to the API key, and `summonr login grok` offers a SuperGrok or X Premium+ sign-in next to its key. A stored sign-in wins over a key; `summonr logout NAME` returns the endpoint to its key. `openai` now speaks the Responses API. A browser login keeps the client id a provider issues on the redirect and accepts a `redirect_uri` with port `0`. Both rows list under Coding plans in `summonr login`, and the `/login` list names the plan beside the row, so typing `chatgpt` or `supergrok` finds it.
- `/login` groups providers as `ready`, `needs attention`, and `not set up`, and on a wide terminal shows a detail pane with the row's key, sign-in, models, and actions. `alt+k` pastes a key on a row that also signs in.
- `/model`, `/login`, and `/usage` fill the terminal and share a tab row; `←` and `→` switch between them (`ctrl+←` and `ctrl+→` also work), and `↑` `↓` change the range in Usage. The frame keeps one size while models load, and a key or sign-in step opens in the side pane beside the list instead of a new dialog. Typing after `/model` or `/login` goes straight into the search field. `/login` lists every provider and each provider's full model list. On a wide terminal `/model` shows the selected model's context, price per 1M tokens, release date, abilities, and the other providers that serve it. Prices are dollar amounts, `tab` filters by ready, favorites, plan, free, think, vision, or a 1M-token context, and a provider that is not ready is one row with its fix; `↵` on an offline provider retries it.
- The `claude-pro` seat is off. Anthropic's terms do not allow a third-party app to sign in with a Claude Pro or Max plan.
- Windows x86_64 release binary and a PowerShell installer (`irm https://summonr.dev/install.ps1 | iex`). Linux binaries are static musl builds. `https://summonr.dev/install.sh` installs on macOS and Linux.
- Every release publishes `SHA256SUMS` and build attestations (`gh attestation verify <file> --repo manaforged/summonr`).
- `/mcp` shows each MCP server's state, tools, schema size, and last error. `summonr mcp list|get|add|remove` does the same from the shell.
- MCP 2025-11-25: summonr answers `roots/list`, shows tool progress, fetches changed tool, prompt, and resource lists, logs server messages to `~/.summonr/logs/mcp.log`, and cancels a call when a turn stops. Servers on an unknown protocol version are refused.
- MCP prompts, resources, and forms: `/server:prompt ARGS` runs a server prompt and `@server:uri` attaches a resource. A server can ask you to fill a form; each field is checked against its schema.
- A server with more than 8 KB of tool schemas is listed under one `mcp` tool, so large servers do not fill the prompt.
- Plugins: `summonr install PATH` installs a plugin in the Claude Code layout (skills, slash commands, agents, MCP servers, `.lsp.json` language servers, hooks, `bin/`); `summonr uninstall NAME` removes it and `[plugins] off` disables plugins. A plugin command runs as `/plugin:name`, or `/name` when the name is unique. A plugin agent is picked by the `task` tool and limited to the tools it lists. Plugin language servers start on first use.
- MCP servers and language servers stop when idle and share one limit: `servers_max` sets how many run at once, and `lsp_idle_secs` sets the language-server idle stop. `/mcp` shows each running server's memory on macOS and Linux.
- Slash-command and `@` mention type-ahead work on the start screen.
- `shift+tab` cycles reasoning effort from none to max. Each vendor and model sends the nearest level it accepts; `reasoning_levels` and `thinking_off` on an endpoint override this.
- The provider table is data. Every built-in endpoint is one `[[preset]]` row in `src/endpoint/presets.toml` (base URL, env keys, wire, reasoning quirks, peer ids), and a row summonr cannot honor fails startup by name. New built-ins: the Qwen coding plans `qwen-plan` and `qwen-plan-cn` and the DashScope hosts `dashscope` and `dashscope-cn`; the MiniMax coding plans `minimax-plan` and `minimax-cn-plan` on the Anthropic wire; `tencent-plan` and `volcengine-plan`; `nvidia`, `huggingface`, `cohere`, `perplexity`, `siliconflow`, `siliconflow-cn`, `ollama-cloud`, and the local `lmstudio`. Coding-plan keys adopt from OpenCode's `auth.json` through their models.dev ids.
- `/goal` with no check configured uses a check detected from the project (`cargo test`, `go test ./...`, or `npm test` when `package.json` has a `test` script) and says how to set another in `.summonr/check.toml`.
- `--json` runs open with `{"type":"session","id":...}`, and the `aborted` event carries a `code`: `time_budget`, `token_budget`, `interrupted`, `cancelled`, or `error`.
- `summonr diff --turn N|last` shows what one turn changed, from the checkpoint at its start to the next turn's checkpoint; resumed sessions keep numbering turns.
- `summonr mcp add` connects once, caches the tool list, and prints the tool count; a server that fails stays configured, `mcp add` exits 1 and names the config file, and `summonr mcp list` shows a clipped `failed: <error>`. In `-p` output each tool line carries a short argument summary (`→ read src/a.rs`), and a call through the `mcp` tool prints `→ mcp server:tool`.
- Hooks take `{ command, dialect = "claude" }` to run a Claude Code hook with its stdin JSON and verdict rules. Every hook gets `SUMMONR_HOME` and `CLAUDE_PROJECT_DIR`.
- `summonr install --list` names each plugin, whether it is on, and what it adds. `summonr login` shows each preset's label, `summonr endpoints` lists set-up endpoints first, and `summonr sessions` prints a header row on a terminal.
- `AGENTS.md` and `CLAUDE.md` expand `@path` lines: the named file, relative to the one holding the line, joins the prompt in place. Files load once across user and project context, and a missing include stays as the literal line.
- `/pane jobs` lists background commands with status, log, and kill. `/pane peers` lists live sessions you can mention with `@name`.
- The diff pane reviews one hunk at a time: `a` accepts, `r` reverts, and `n` adds a note for the model (Enter saves). Typed letters stay in the pane, and `esc` returns to the composer. A pane narrower than `diff_split_min_cols` shows the unified diff. Marks persist in `.git/summonr/review.json`.
- Inline terminal mode, grouped repeated tool calls, a transcript scrollbar, and images attached during a turn.
- `summonr login NAME --url URL [--model M]` adds your own OpenAI-compatible server, or updates it in place; when the server lists one model, summonr picks it. `summonr logout NAME --remove` deletes it, forgets its key, and clears `default` and `default_model` when they named it. An unknown endpoint name suggests this command.
- `summonr login` prints one compact line per endpoint with its state and label, set-up endpoints first. A preset `label` names the region and default model, `start` names the command that starts a local server, and `key_from` lets rows on the same plan share a stored key (`zai` and `glm-intl`, `zai-cn` and `glm`). A keyless endpoint with no model reads `needs model`.
- A misspelled endpoint name of `typo_min_chars` characters or more gets a hint for one changed character or two swapped ones.
- A `--steps` item that says ``Run `CMD` `` gets `CMD` as its own check.
- A hook table takes `matcher`, with the syntax of a Claude Code `hooks.json` matcher, checked against the tool name in the hook's dialect.
- The composer header shows `model (effort) · endpoint`.

### Changed

- Plan mode is gone: `/plan`, `/plan accept`, `/plan reject`, the `plan_mode` setting, the `plan` workflow role, and `--plan` are removed. `/goal`, `--goal`, and `--steps` work as before, and after a goal ends the session returns to its earlier mode. A `plan_mode` key left in `config.toml` is ignored without a warning and dropped on the next save.
- Search is its own `search` tool. `pattern` searches contents and `glob` alone finds files by name, under any readable path, ignoring case unless `case_sensitive` is set. A call returns what it found within `search_secs`. `read` no longer searches, and `read` and `search` reach paths outside the workspace except protected ones.
- `summonr login NAME --model M` changes the model of a server you added, with no `--url`. `summonr login NAME --default [--model M]` makes any endpoint that is set up the default. `summonr login NAME --help`, and `--help` after `logout`, `diff`, `profile`, `install`, `export`, `doctor`, `classify`, `worktrees`, and `mcp`, prints that command's help.
- `summonr login` and `summonr endpoints` size their columns to the longest value, show only the URL on a row with no model, and say `1 model`. Each built-in label names its default model and the row whose key it shares; `glm` names `GLM_CODING_API_KEY` and `glm-intl` names `ZAI_CODING_API_KEY` when the key is missing.
- Every session id on screen is the short id, and `--session` accepts it.
- `summonr diff --turn N` writes the `--stat` summary line to stderr before the patch.
- An approval prompt reads `<command> — <reason>`.
- In `--steps --json`, a step's `start` event carries `check`: the step's `Run` commands joined with ` && `, or null.
- Enter on a suggestion runs it, prompt rows too; Tab completes. With an empty composer and no running turn, `esc` closes the open side pane. Accepting a plan prints the check the work uses. A review note opens under its hunk header. Task cards use workspace-relative paths, a kept child checkout shows how to remove it, and with no usable endpoint the composer header reads `no provider · /login`.
- Sign-in tokens are stored apart from API keys, so signing in no longer overwrites a key. A sign-in made with an earlier build is not read; sign in again once.
- Every dialog, inline prompt, and key hint is built from one UI kit: dialogs share one frame and always end in their key hints, approval and form prompts look the same in the terminal and the desktop window, and hints show your own key bindings with one separator style.
- Commands run without a prompt, sandboxed or not. Only irreversible destruction asks first: wiping the system, home, or project root, `mkfs`, `dd` to a device, `terraform destroy`, cluster deletes, and dropped tables, including inside a heredoc fed to a shell. `sudo`, force pushes, pipes into a shell, git discards, and protected-path writes no longer ask; `confirm = "risky"` in `policy.toml` brings those prompts back. `--strict` refuses the whole list, and your own `[policy] deny` rules stay hard refusals under `--sandbox` or `--strict`.
- summonr starts when its home is not writable, such as inside another tool's sandbox: the session lives in the temp directory, says so, and does not resume. A keychain key that cannot move to the key file still works.
- macOS release binaries are Apple silicon only. The TLS stack does not build for Intel Macs, so `install.sh` stops there with a clear message instead of a failed download.
- Tool calls take one line each: `● Read path 1-40`, `● Run command` with its output under `⎿`, and `● Explored 3 files, 1 search` for a run of reads. A failed call turns the dot red and names its exit code. An edit shows its diff right under the call, split or unified.
- Research children run shell commands in a read-only sandbox, so they can run checks and scripts instead of replicating them with reads; `read` with `op=search` takes `paths` to search several paths.
- Child tasks: children the turn waits on show inline under their task call, one line each with their latest action, and fold to their result when done; background children run in a strip above the composer and report in the transcript when they finish. `ctrl+a` lists every child with its id, access, time, and tokens.
- During a turn, `enter` queues a message and `enter` on an empty composer sends the queue into the running turn. Command suggestions work during a turn; settings and pages open at once, and other commands run when the turn ends.
- `notify = "off" | "turn" | "all"` (the Notify row in `/settings`): desktop notifications when a turn ends and, with `all`, when an approval waits, only while the terminal is not focused.
- Three themes: `space`, `nord`, and `dawn`, a low-glare light theme; `theme = "auto"` picks `space` or `dawn` from the terminal background. Configs that name a retired theme still load.
- `ctrl+n` folds every thinking block, then every tool output, then shows them all again; `tools = "hidden"` keeps only the call lines.
- The footer shows the session's spend beside the context and cache meters when the model has a price.
- `background = "terminal"` in `config.toml`, or the Background row in `/settings`, uses the terminal's own background instead of the theme's.
- `/usage` is a full-screen page: tokens, cache hit, sessions, and cost for 24h, 7d, 30d, or all time, a daily chart, and breakdowns by model, workspace, and hour of day.
- The `mcp` module is on by default. A server starts on its first tool call and stops after two idle minutes, so configured servers use no memory until called. `tools`, `idle_secs`, `keep_alive`, and `timeout_secs` on `[mcp.NAME]` tune a server.
- MCP calls follow `policy.toml` and `--strict`, and stdio servers run inside `--sandbox`. A reply over 4 MiB is dropped; output over 50 KB is cut and the full text saved to a file.
- One permission mode: everything runs without a prompt. `/permissions approve` is removed.
- `sudo` asks for the password in a dialog through `SUDO_ASKPASS`.
- Fewer wasted rounds: command output keeps about 16 KB, `write` over a file returns one line, `read` pages by 50 KB, and batch edits to the same line apply in order.
- A foreground command that reaches the wait limit continues as a background job unless `timeout_secs` was set.
- `todo` is advertised only in steps and goal runs, `task` only in interactive sessions, and the `search` module is off by default.
- Seat order: the endpoint argument, `SUMMONR_ENDPOINT`, `default` in `config.toml`, the last seat used, then the first endpoint that is set up. A seat is remembered in `recent.json` only after a turn succeeds, and the old `last_endpoint` key is removed on the next save. A `default` or last-used endpoint that is gone or has no key prints one note and falls back. Effort is remembered per seat; a reasoning model with no effort set uses `high`.
- DeepSeek and z.ai GLM receive earlier reasoning on every turn, as both vendors require.
- In `--goal` and `--steps`, the check decides the exit code: a run that stops short runs the check once at exit and exits 2 when it fails, even after a budget ran out. The `work` event's `checks_failed` and the `verify` events now agree.
- The plan is saved as `plan.md` in the session directory instead of `.summonr/plan.md` in the workspace.
- Lower startup time, memory, and per-turn overhead: model facts come from the local catalog, requests are encoded once per turn, and waits no longer poll.
- The approval prompt shows the whole command, wrapped, and says why it needs approval, for example `a destructive command because …`.
- `summonr diff --turn N` reads `turn N of session <short id>`, and its JSON adds `session`.
- A child whose edits conflict with the parent shows as a conflict with its kept checkout (`kept <repo>-deltas/<id>: conflicts with the parent`), not as failed. summonr never deletes a kept checkout. A child row names the plugin agent it runs as.
- Hunk review marks live in the repository's git directory, `.git/summonr/review.json`.
- In plan mode the model ends with the plan and does not ask for permission; `/plan accept` or a short reply accepts it.

### Fixed

- Cancelling a turn no longer kills background children. A `task` call made with `background: true` keeps running when the turn is cancelled, stays `running` in the journal, and a later poll collects its result; stopping the session or `kill: true` still stops it, and children spawned without `background` still stop with their turn.
- A `tags` search hit on a line over 500 bytes, or inside a multiline match, now carries the tag `edit` accepts, and searching overlapping paths with `depth` no longer drops the deeper path.
- Secret values no longer reach the model in any session. A tool result shows `[secret]` in place of each secret value from the workspace's `.env` files, `.npmrc`, `.pypirc`, `~/.aws/credentials`, `~/.netrc`, `~/.git-credentials`, `~/.config/gh/hosts.yml`, and `~/.docker/config.json`, and in place of any PEM private key, whether it came from `grep -r`, `cat`, `read`, or any other command. Commands still read the real files, so test suites that load `.env` keep working. `search` never reads a protected file, even uncontained, and `credentials.*`, `.git-credentials`, `.pypirc`, `~/.docker`, `~/.kube`, `~/.config/gh`, and `~/.config/gcloud` are protected.
- In the terminal, only the `✻ thinking` header folds expanded reasoning, so a drag over the reasoning text selects and copies it.
- A read-only command such as `grep` that times out no longer reports that it may have changed workspace files, and a command that redirects into a file still does. `mdfind` and `locate` count as read-only.
- A command run with `workdir`, or after `cd`, can no longer read a protected path such as `~/.ssh` by a relative name, and `cp`, `mv`, `ln`, `install`, and `rsync` can no longer copy one out. A numbered redirect such as `3> FILE` counts as a write.
- In a contained session, a shell content search such as `grep -r`, `rg`, `ag`, or `ack`, or a copy, over a folder that holds a protected path such as `~` or `/` is refused, so it can no longer print `~/.ssh`, `~/.aws`, or summonr session files. `~` and `$HOME` count as the home folder in command checks. A name search with `find`, `fd`, `mdfind`, or `locate` still runs.
- A name search lists symbolic links to files, results say they are newest first, and a result with few or no hits names what it skipped. A Claude hook sees a name search as `Glob`.
- A budget counts from the run that loads the session, so a resumed or forked session gets its full `--max-secs` and `--max-tokens` again.
- Reverting a hunk in the diff pane tells the next turn, so the model does not report the reverted code as still in place.
- `-p` labels an MCP tool lookup as `mcp search QUERY`, so lookups no longer read as duplicate calls. The banner and the resume line show the short session id.
- The control files `.summonr/check.toml` and `.summonr/policy.toml` are not part of a Work snapshot, so a check file summonr restored never reads as a workspace change.
- The turn receipt's dirty count ignores caches and `.summonr` state. A file a `summonr diff` wrote stays out of later sessions' turn diffs, and a change to `.summonr/check.toml` or `.summonr/policy.toml` is never a touched path.
- The deferred `mcp` tool accepts arguments as an object, a JSON string, or fields beside `tool`.
- The model cannot write `.summonr/check.toml` or `.summonr/policy.toml`; a check file that changes during a run gets a note, and the check loaded at start stays in force. A file that summonr's own output is redirected to never counts as a change.
- `summonr diff` and child landings never include generated caches (`__pycache__`, `*.pyc`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`) or summonr state under `.summonr/`, even without a `.gitignore`.
- A child never inherits goal or steps mode, a child of a plan-mode parent is read-only, and a child's first message names only its own checkout.
- A local server needs no `api_key`. When `ollama` or `lmstudio` is not running, the error gives the start command and, with no model set, lists the server's models. `summonr logout NAME` says what it removed. A closest-name hint appears only within `closest_percent` of the name length.
- An unknown slash command names the closest command and never runs another one. One `esc` closes `/login` and `/model`, and the `ctx` meter hides when the context window is unknown.
- `--goal` and `--steps` from the command line run the completion check and the goal judge. Before, a goal that was already met could repeat turns until its budget ran out.
- Dragging across the transcript in a terminal copies exactly the text crossed, in either direction, and the footer says "copied N lines". Before, the first character was dropped, a drag toward the start copied nothing, and nothing confirmed the copy.
- Links in replies open on click, a dropped file lands only in the pane under the pointer, and loading older messages keeps the message you were reading in place.
- Review follow-ups: `xargs -I {} sh -c {}` is seen by the ask gate; a failed attempt's partial reply is no longer sent back as the last message; the password dialog names the program that really asked; a hook's process group is stopped only on timeout; compaction keeps a later message that repeats the turn's first prompt and says when it starts.
- Terminal UI polish: the approval and form bands render on their own rows instead of over the footer; approvals list their keys (y allow · Y always · n deny); `?` in an empty composer opens the shortcuts and `↑` brings back a draft cleared with ctrl+c; counts read "1 hunk" and "3 hunks"; long times read 12m34s; key hints follow your hotkeys; pane titles ellipsize instead of hiding the close button, and a narrow terminal says how wide panes need; Nord and Dawn hints and code blocks are readable; empty states, separators, and wording match across panes, search, usage, and messages.
- Terminal UI: tool output can no longer move the cursor or ring the bell; typing y or n while an approval waits goes into the draft; ctrl+a and ctrl+x select and cut in a non-empty composer on Linux and Windows, and ctrl+u, ctrl+k, and ctrl+w can no longer be bound as hotkeys; child lines render the same in the strip and the transcript.
- Tools: a pre-tool hook that fails or times out blocks the call and its helpers are stopped; tags on lines longer than 8 KB match their edits; MCP over HTTP delivers progress as it arrives, refuses an event over the frame cap, and cancels a stopped call on the server.
- Providers: a long tool-argument stream no longer trips the stall timeout; a rejected OAuth token is refreshed instead of reused; OAuth errors never echo the token reply; cached tokens without a listed rate are priced at the input rate; prompt-cache writes are priced at the model's cache-write rate (Anthropic bills them above input); an environment key is not sent to a base URL you overrode; Claude 3 models other than 3.7 get no thinking budget.
- Sessions: auto-compaction no longer keeps the current prompt twice; a failed model round with partial text keeps history in step for later repairs; a refused dangerous command asks again next time; cancelling a turn stops its background jobs; a session with one bad log line still resumes; an aborted child reports its real reason.
- Sandbox and policy: `--strict` refuses an allow-listed command that destroys work, such as `git reset --hard`; git hooks, config, and commondir stay protected inside submodules too; the ask gate sees quoted verbs, `eval`, `find -exec rm`, `bash -c --`, and `xargs sh -c`; a password prompt names the program that asked for it.
- MCP: a timed-out call no longer corrupts the next one, server start waits at most 60 seconds, remote servers reconnect after their session expires, and Ctrl-C stops servers and commands.
- Providers dialog: probe results fill each row's models without opening Models first and update its status in place, so rows no longer move while the dialog is open; Enter works on a keyless row, and a local row such as `ollama` reads "none needed" for its key instead of asking for one; a login or logout on the active seat applies at once; a key pasted on `openai` is checked against the API, not the ChatGPT sign-in; a stream failure keeps its error message.
- A stream that sends only keep-alives fails after the idle timeout and is retried, instead of holding the turn.
- A hook that writes more than 64 KB no longer stalls the tool call.
- Leaving plan mode removes the sandbox plan mode turned on.
- A failed compaction summary retries after 8 rounds.
- A file whose change time moved is hashed again for undo snapshots.
- A closed stdout or stderr pipe ends summonr quietly and leaves no crash report for the next run.
- The `edit` and `write` tools refuse `.summonr/check.toml` and `.summonr/policy.toml` in every mode. When the check file changes during a run anyway, summonr restores it and adds a note.
- A file that summonr's own output is redirected to stays out of every checkpoint, so `summonr diff --turn` never lists it.

### Security

- A plan sign-in must return the nonce summonr sent and grant plan access, and a redirect that names a different client than the one on file is refused.
- A workspace `SYSTEM.md` loads only when the workspace is trusted.
- `~/.summonr` is created with mode `0700`, and existing directories are tightened to it.
- A non-empty `policy.toml` in an uncontained session warns at startup that its rules are not enforced.
- `Endpoint` debug output shows only whether a key or header is set, never its value.

## 0.1.0-rc.1 - 2026-09-23

### Added

- Release binaries for macOS (arm64, x86_64) and Linux (x86_64, arm64), each with a SHA-256 file, and `scripts/install.sh` to download, check, and install one.
- The `/model` picker opens from cache in about 0.3 s and checks each provider in the background. It groups current, pinned (`alt+p`), recent, and providers, and shows one state word per provider with a one-line fix. `F5` rechecks a failed provider. `summonr endpoints` uses the same state words and the same discovery path.
- Sign-in happens inside the dialog. A pasted key is checked against the provider before it is saved. OAuth shows the device URL, code, and elapsed time. `/login` filters and groups providers, `/login NAME` opens the key prompt, and `alt+d` signs out.
- Endpoints read keys from OpenCode's `auth.json` and Kimi's `kimi-code.json`. `summonr auth` names the source.
- Built-in endpoints `together`, `groq`, `cerebras`, `mistral`, `muse`, `zen`, `go`, and `opencode`. `responses` is a selectable wire.
- A sandboxed write or network denial asks `[ allow ]`, `[ always ]`, or `[ deny ]`.
- `web` fetches as markdown, text, or html.
- A fenced `chart` block, or mermaid `xychart`, renders as labeled bars.
- `theme` accepts `space`, `fantasy`, `nord`, `gruvbox`, `solarized`, and `light`.
- CLI verbs `summonr resume`, `summonr install`, and `summonr doctor --logs`.

### Changed

- summonr depends on the one Sigil package `sigil-ui` (feature `terminal`) instead of `sigil-gui` plus `sigil-tui`.
- The terminal interface runs on Sigil's terminal loop. Sigil restores the terminal on exit, panic, and SIGTERM, SIGHUP, or SIGINT.
- The `/` popup and `/help` list 30 commands, grouped by job. Old names still work for one release; see Renamed commands.
- `ctrl+d` only quits. `alt+d` deletes a session in the sessions picker, signs out in `/login`, and sets the default in `/model`. `alt+p` pins a model. `F5` is the only picker refresh; `ctrl+r` stays prompt history.
- `↑` on an empty draft pulls queued input back; `ctrl+q` no longer does.
- A key stored with `summonr login` wins over peer keys. An endpoint with no key fails with the missing-key message.
- The model sees five core tools: `read`, `edit`, `write`, `run_command`, and `todo`. Modules add `skill`, `lsp`, `web`, `task`, and `bus`. `read` search takes `literal`, `caseSensitive`, and `include`; `edit` and `write` take a `files` batch.
- Chat runs at once. `shift+tab` and `/plan` enter plan mode, which is read-only and saves the plan to `plan.md` in the session directory. `/plan accept` runs the plan in the same turn.
- The `task` module is off by default.
- `--goal`, `--steps`, and `implement` children edit the launch directory. `--worktree` isolates the session.
- Sandboxed network is on by default; `--no-net` denies it.
- Anthropic: models before Claude 4.6 get a manual thinking budget, newer models get adaptive thinking. Defaults are `claude-sonnet-5` and `claude-opus-5-5`.
- Gemini tool schemas use `parametersJsonSchema`. Effort maps to `thinkingBudget` on 2.5 and `thinkingLevel` on 3.
- Dialogs size from the terminal and repaint on resize. Copy is `ctrl+b` everywhere.
- A malformed `config.toml` stops startup with one error.
- `src/limits.rs` holds every timeout. Transport retries run in leyline's `RetryPolicy`. A stream cut mid-body retries once.
- The per-turn git snapshot runs in the background. The transcript repaints only changed entries, and an idle session skips paints.
- Multi-path `read` shares one 50 KB cap. The tool schema is frozen for the session.
- Release builds use fat LTO, one codegen unit, and strip.
- Crate and command are `summonr`. `summon` and `summoner` stay PATH aliases.

### Fixed

- Anthropic tool results carry `is_error` and images. Usage counts cache reads and the final `message_delta`. Refusals show the provider text.
- Gemini keeps text-part signatures, reports blocked prompts, sends the real function name, and honors `retryDelay` on 429.
- Responses shows stream errors, streams commentary and reasoning summaries as reasoning, and keeps a `max_output_tokens` stop as a length stop. Resumed sessions keep message phase.
- Error frames and cut streams fail instead of returning a partial reply. Thinking replays only to the model that produced it.
- `--json` keeps its event order, and an abort emits Aborted then Receipt.
- Compaction and tool-argument repair are saved in the journal, and compaction resets read dedup.
- Plan mode entered mid-session turns on the command sandbox.
- Clipboard paste attaches the full image on macOS.

### Removed

- `--max-turns`. Use `--max-tokens` or `--max-secs`.
- The header fps readout and the separate reasoning band.
- AppKit from the default terminal build.

### Renamed commands

| Old | New |
| --- | --- |
| `/auth` | `/login` |
| `/quit` | `/exit` |
| `/effort` | `/model` |
| `/rollback` | `/undo files` |
| `/branch` | `/fork` |
| `/tree` | `/resume` |
| `/session`, `/stats` | `/status` |
| `/profile` | `/usage session` |
| `/net` | `/permissions net` |
| `/write` | `/permissions write` |
| `/layout` | `/pane` |
| `/theme`, `/tools`, `/reasoning`, `/receipts`, `/bus`, `/modules` | `/settings` |
