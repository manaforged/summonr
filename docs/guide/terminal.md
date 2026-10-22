# Using the terminal

This page covers the summonr screen: sending and steering turns, panes, reviewing changes, sessions, child sessions, mail, memory, web search and the browser, and the desktop window. Every key binding is in the [Keys](reference.md#keys) table, and every slash command is in [Commands](reference.md#commands).

## The screen

The screen is a set of panes on one session. The chat and the composer are always there. A side pane can show the files tree, the plan ledger, the diff, or a file view.

The footer shows the workspace and branch on the left. On the right it shows the session's spend (when the model has a price or the provider reports a cost), then the context and cache meters. The `ctx` meter hides when the model's context window is unknown. Both meters appear once the first reply reports usage, and the cache meter counts the session's children too. With no usable endpoint, the composer header reads `no provider · /login`.

A `price` on the endpoint prices every round. On the Anthropic wire, `cache_ttl` sets the prompt-cache lifetime: a cache write costs 2x the input price at `1h` and 1.25x at `5m`, and a read costs 0.1x either way. `/usage` shows token spend, and `/usage session` profiles this session; `↑` `↓` change the range.

The terminal title reads `summonr · <workspace> · working` during a turn and `· ready` between turns. On macOS the screen shows `alt` as `⌥`, so `alt+d` reads `⌥D`.

## The command list

Typing `/` opens the command list above the composer. Each row shows the `/command` and its argument. After `/model ` or `/login `, the list shows the values that command takes: the endpoint's cached models or the endpoint names. `↑` `↓` move, `tab` completes, `enter` runs the row, and `esc` hides the list. An unknown command names the closest one and never runs another.

`/login` groups providers under `ready`, `needs attention`, and `not set up`. `/settings` has a `NAME tools` row for each MCP server.

## Send and steer a turn

While a turn runs, a row above the composer shows a spinner, what the model is doing (`thinking`, or the tool it runs), the round, the seconds so far, the output rate, and the queue count. The composer status counts waiting input, as in `2 queued, ↑ edits`.

A message you send during a turn goes into the running turn. The model reads it after its current tool calls. The message waits in the queue for the next turn instead when:

- an approval or a question is open,
- earlier messages or commands still wait, or
- it starts with `!` or `/`.

Press `enter` on an empty composer to send the waiting plain messages into the running turn.

A draft that starts with `!` runs as a shell command before any model round. `!cmd` shows the output as a tool card, then sends `$ cmd` plus the output as the prompt. `!!cmd` shows the output only. `esc` cancels the command and stops its process group.

A paste of eight or more lines, or 1,000 or more bytes, folds into `[pasted N lines #k]` in the draft. summonr expands each token back to the pasted text when the prompt is sent or queued; `ctrl+c` discards them with the draft.

## Images and diagrams

On macOS, `ctrl+v` or `alt+v` attaches an image from the clipboard; the desktop window also takes `cmd+v`. A pasted image, one attached with `@shot.png`, and any image the model shows you appear in the transcript. So does a markdown image link such as `![plot](out/plot.png)` in a reply. `/view shot.png`, or `→` on an image in the tree, shows it in the view pane.

Ghostty, Kitty, WezTerm, and Konsole render pictures through the Kitty graphics protocol, iTerm2 and Warp through inline images, and other terminals as half-block cells. `SIGIL_GRAPHICS=kitty`, `iterm2`, or `none` overrides the detection, and tmux turns it off. A picture is at most 48 rows tall.

A fenced `mermaid` block in a reply renders as a diagram: flowcharts, state and sequence diagrams, pie charts as bars, and `xychart` or `xychart-beta` as labeled bars. A fenced `chart` block with `label value` rows draws the same bars. Other Mermaid kinds show their source in a frame.

The output rate is tokens per second. While text streams it is an estimate: characters received in the last three seconds, divided by the characters per token measured on the previous round (4 until then). When the provider reports the round's usage, the rate becomes exact: output tokens over the time between the first and last streamed chunk, or over the whole round when nothing streamed, as in a round that only called tools.

## Panes

| Pane | Open with |
| --- | --- |
| Files tree | `ctrl+x e` or `/pane files` |
| Plan ledger | `ctrl+t` |
| Diff | `ctrl+x d`, `/pane diff`, or `/diff` |
| File view | `→` on a file in the tree, `/view PATH`, or view file in the tree's menu |
| Worktrees | `/pane worktrees` |
| Jobs | `/pane jobs` |

`ctrl+x b` hides or restores the pane, and `/pane off` closes it. With the composer focused, an empty draft, no open overlay, and no running turn, `esc` closes the open side pane.

In the files tree, `ctrl+→` focuses it, `↑↓` move, `←→` collapse or expand, `PgUp/PgDn/Home/End` jump, and `ctrl+←` or `esc` returns to the composer. Enter opens a directory or attaches `@path` to your prompt. A right-click opens a menu: attach, open in `$EDITOR`, insert path. The tree, the diff, and the view follow the file system: a change on disk relists, re-diffs, or reloads the pane within about half a second.

### Worktrees and jobs

`/pane worktrees` lists this repository's `summonr/*` worktrees: branch, commits ahead of the base, dirty or clean, the session that ran there, and the directory. Enter or `→` opens its diff. The right-click menu offers show diff, merge (fast-forward only; a dirty tree is refused), remove worktree (only when clean and fully merged), and insert path. `summonr worktrees` prints the same rows, `--json` as JSON, and `summonr worktrees prune --json` prints the removed paths as a JSON array.

`/pane jobs` lists this workspace's background commands. Each row is the command and its state: `running`, `exit N`, `killed`, or `dead`. Enter or `→` expands its status lines. The right-click menu offers view log (the job's `.log` in the view pane) and kill job, the same stop as `run_command` with `{"job": ID, "kill": true}`.

### Layouts

| Command | Effect |
| --- | --- |
| `/pane focus`, `/pane run`, `/pane review` | Preset layouts |
| `/pane "chat \| files 30%"` | Set the split |
| ctrl+shift+←/→ | Resize the split |

The layout persists in `~/.summonr/config.toml` as `layout`. `ctrl+o` opens the settings modal, where every hotkey is editable; your choices go under `[hotkeys]` in the same file.

`/settings` switches tool output between compact and full. Chain of thought is `collapsed` (the default: one dim line with a line count), `expanded`, or `off`. Turn receipts are `off` (the default), `collapsed` (one line: files read, searches, commands, failures, changed files, and the check result), or `expanded` (adds the start commit, the first five failures and changed files, and each check). A click on a receipt toggles it.

`ctrl+x n` folds every thinking block, then every tool output (`tools = "hidden"`), then shows them all again. A click still opens one block.

## Review changes

The diff pane shows the changes against the base: the worktree base when you passed `--worktree`, `HEAD` otherwise. Each row is `+added −removed path`. `/diff PATH` jumps to one file.

Enter or `→` opens a file's hunks, one card per `@@` block, each with a `pending`, `accepted`, or `rejected` badge.

| Key | In the diff pane |
| --- | --- |
| `↑` `↓` | Pick a hunk |
| `a` | Toggle `accepted` (Enter does not) |
| `r` | Revert the hunk (`git apply --reverse` of just that block) |
| `n` | Open a note field under the hunk header; Enter saves, `esc` cancels |
| `t` | Keep only the files the last turn edited (the header says `· last turn`) |
| `c` | Copy the patch on show to the clipboard (OSC 52 in the terminal) |
| `←` or `esc` | Back to the file list; from the list, back to the composer |

A saved note rides to the model with your next prompt. While the pane has focus, typed letters stay in the pane. The right-click menu offers the same actions. Verdicts and notes persist in `.git/summonr/review.json`, or in `.summonr/review.json` outside a git repository.

A pane `diff_split_min_cols` columns or wider (100 by default) shows a file's hunks side by side, old on the left. A narrower pane shows the unified diff.

### From the command line

The same data is `summonr diff [PATH]` (the patch), `summonr diff --stat`, and `summonr diff --json`. `summonr diff` never lists generated caches (`__pycache__/`, `*.pyc`, `.pytest_cache/`, `.mypy_cache/`, `.ruff_cache/`) or summonr state under `.summonr/` other than `check.toml` and `policy.toml`, even without a `.gitignore`.

`summonr diff --turn N` shows what turn N changed: from the checkpoint taken at its start to the next turn's checkpoint, or to the workspace for the newest turn.

| `--turn` value | Shows |
| --- | --- |
| `N` | Turn N; the summary reads `turn N of session <short id>` |
| `last` | The newest turn that changed files, in the newest session for this workspace |
| `session` | Every turn of that session; the summary reads `all turns of session <short id>` |

Without `--stat` or `--json`, it writes the same summary line as `--stat` to stderr before the patch. The JSON has `base: "turn N"` and `session` with the session id. A resumed session keeps numbering its turns. A diff of the whole workspace still reads `vs HEAD`.

A file that summonr's own output is redirected to stays out of every later checkpoint in that workspace, in every session, so `--turn` never lists it. A change to `.summonr/check.toml` or `.summonr/policy.toml` is never a touched path.

## View and edit a file

The view pane shows the file with line numbers; lines the diff added are green with a `+`. `↑↓`, `PgUp/PgDn`, and `Home/End` scroll, `o` opens the file in `$EDITOR`, and `esc` returns to the tree.

Press `e` to edit in place. The keyboard goes to the file (type, Enter, arrows, Home/End), and the caret is the inverted cell. `ctrl+s` saves and `esc` leaves. With unsaved text, a second `esc` discards it.

## Sessions

| Task | Command |
| --- | --- |
| Name a session | `summonr --name TEXT` or `/name TEXT` |
| Resume a session | `--session` with a name, the short id, or an id prefix |
| List sessions | `summonr sessions` (names and short ids, with a header row on a terminal) |
| Search past sessions | `summonr sessions --grep TEXT` |
| Export a transcript as Markdown | `summonr export [ID\|NAME]`, or `/export [PATH]` in the session |
| Show the binary path and crashed or incomplete runs | `summonr doctor` |
| List the last 20 per-run diagnostic logs | `summonr doctor --logs` |

`/export` writes to `~/.summonr/exports/<id>.md`, and `/export PATH` writes where you say; it works while a turn runs. `ctrl+x y` copies the latest reply and `/copy all` the full transcript, through OSC 52 in the terminal. `ctrl+x t` releases the mouse so you can select any span; press it again to restore it.

Every place that shows a session id shows the short id, its first 12 characters. They encode the start time, so two sessions started in the same millisecond share one; resuming by it then lists the full ids to choose from.

`/undo` continues in a copy of the session with a new id, so `summonr sessions` lists both.

`summonr sessions --grep TEXT` searches past sessions newest first, case-insensitively. It prints the session id, its start time, its directory, and each matching line, and exits 1 when nothing matches. The model can run it through `run_command` to recall earlier work.

Diagnostic logs older than 14 days are removed unless their process is still active. Session directories under `~/.summonr/sessions/` with no file change for 30 days are removed once per start, except the session that run opens. Set another period in days, 1 or more, in `~/.summonr/config.toml`:

```toml
[sessions]
retain_days = 90
```

## Child sessions

The `task` tool delegates work to fresh child sessions. Only an interactive session offers it; `-p`, `--steps`, `--goal`, and `summonr acp` runs have no steering channel, so the model does not get `task` there. The model can emit several `task` calls in one response, and summonr runs them concurrently. Every child tool call, result, and answer streams into the parent transcript under the child id.

| Kind | Workspace | Can |
| --- | --- | --- |
| `implement` | An isolated checkout in `<repo>-children/<id>` that shares the parent's `target/` and `node_modules` | Use the parent's file, shell, and language-server tools; background commands stay with the parent |
| `research` | The parent workspace | Read and search only; no shell, no writes, no background commands |

The `task` module is on by default. Turn it off in the Modules rows of `/settings`, or with `task = false` under `[modules]` in `~/.summonr/config.toml`.

### Implement children

The parent applies each finished child's edits on its own, then verifies. Generated files (`__pycache__`, `*.pyc`, and the pytest, mypy, and ruff caches) and summonr state under `.summonr/` other than `check.toml` and `policy.toml` never land. The receipt lists the files that landed.

A child never inherits goal or steps mode: it works in plain working mode. Its first message names only its own checkout, and children started together do not see each other's edits.

When two children change the same lines, that child's checkout stays on disk and the others still land. The child shows as a conflict, not a failure: `kept <repo>-children/<id>: conflicts with the parent`, followed by an `rm -rf` hint that removes the checkout when you are done. summonr never deletes a kept checkout. Pass `--worktree` on the session to isolate the whole run in a git worktree.

### Limits and budgets

Each child uses the endpoint context window. If the parent has a token or time budget, the children share what remains of it; otherwise a child runs until it finishes or you cancel it. A child gets the parent's system prompt and tool list byte for byte, so its first request reuses the parent's prompt cache; its instructions and working directory arrive in its first message.

A child that calls `task`, `web`, an MCP tool, or peer mail gets a refusal. A research child that calls `edit` or `write` is told it is read-only. A research child that finished before you pressed Esc keeps its result: the model reads it on the next turn instead of an `aborted` answer.

A `task` call can name a plugin agent with `agent`. Its instructions open the child's first message, its `tools` list limits what the child can call, and the child's row names that agent. Task card titles use workspace-relative paths, and a card with one child takes that child's title.

### Background children

| Call | Effect |
| --- | --- |
| `task` with `background: true` | Starts the child and returns its id at once |
| `task` with `child: ID` | Reports state, elapsed time, and last action while it runs; the full result once it finishes |
| `task` with `child: ID` and `kill: true` | Stops the child |

A turn does not end while a background child runs. summonr waits, then defers the turn the way it does for a background command, until the child finishes or the model kills it. **Clear** on a background status row dismisses that row for the current view without stopping the child; its completion report and session log stay available, and new children still appear.

## Talk to other sessions

Agent sessions on this machine share a mailbox: files under `~/.summonr/mail`, with a roster, one inbox per session, and one board per workspace. There is no server; each session appends and polls files. Mail is peer text, never user instructions, and each session's own permissions still apply.

Every summonr session joins the roster under the name you set with `--name` or `/name`, or else the workspace folder and a number, such as `summonr-1`. Child sessions stay off the roster, and `SUMMONR_MAIL=off` keeps a session off it. `/agents` lists the sessions on the roster.

To message a session:

1. Type `@` in the composer and pick the session, or type its name: `@summonr-2 the schema landed`.
2. Send. A prompt that names a live session goes to that session's inbox instead of to the model.
3. The mail arrives there as `[mail from @summonr-1 …]`. `[agent] mail` in settings decides whether it starts a turn or waits for the next one. The other session replies the same way, with `@summonr-1`.

The `mail` module, off by default, also gives the model the tools `agents`, `send`, `inbox`, and `board`. `send` takes a name, an id, or `all`.

### From another agent

The mailbox is also a CLI, so any agent CLI (Claude Code, Codex, a script) can take part:

```
summonr mail                  # who is on this machine
summonr mail send <name|id|all> <text>
summonr mail inbox <id> [--clear]
summonr mail board [get <key>|set <key> <text>]
```

A foreign agent picks an id and reads `summonr mail inbox <id>`; anyone can write to it with `summonr mail send <id> <text>`. Its messages arrive from `cli` unless it writes its own inbox name into replies.

`inbox <id> --clear` prints the mail and empties that inbox; it refuses the inbox of a live summonr session, because that session keeps its own read position. A lock file next to each inbox makes a send, a read, and a clear wait for each other, so no message is lost.

## Memory

summonr remembers across sessions through two Markdown files that load into every trusted session:

| Memory | File |
| --- | --- |
| User | `~/.summonr/memory/MEMORY.md` |
| Project | `~/.summonr/memory/<name>-<hash>/MEMORY.md` |

`<name>` is the repository folder and `<hash>` comes from its first commit, so moving, renaming, or cloning a repository keeps its memory. A folder with no commits is keyed by its path. Every worktree and child checkout of one repository shares its project memory; another repository never sees it.

Ask the model to remember something and it writes the file with its normal edit tools, one dated entry per fact. It is told to save corrections, stable preferences, and facts the code does not show, never what the repository already records, and to check a remembered fact against the code before it acts on it.

Edit or delete the files yourself at any time. `/memory` shows where they are, how full they are, and whether memory is on. Changes apply to new sessions.

Each file loads up to `memory_bytes` (25,000 bytes). A longer file loads its newest entries, under a note that tells the model the older part is not shown and to read the whole file before it edits it, so a prune never deletes what the model did not see. Invisible and direction-changing characters are dropped, and secret-shaped values become `[secret]` before the text reaches the model.

Memory loads only in a trusted workspace (`--no-trust` skips it), and `memory = false` under `[modules]` turns it off. Under `--strict` the summonr home is protected, so the model reads memory from its prompt but cannot write it.

## Questions from the model

In an interactive session the model can call `ask` to put one to four questions to you. The dialog above the composer shows one question at a time.

| Input | Effect |
| --- | --- |
| `↑` `↓` | Move between options |
| `space` | Toggle an option when several are allowed |
| `enter` | Pick |
| Text in the composer | Your own answer; a number picks that option |
| `y`, `e`, `esc` on the review | Send, edit, or cancel |

One question sends at once; several end on the review. A message you sent into the turn before the dialog opened never answers it; it reaches the model after the dialog closes. The same dialog shows MCP server forms. `-p`, `--steps`, `--goal`, `summonr acp`, and child sessions have no one to answer, so the model does not get `ask` there.

## Notifications

When a turn ends while the terminal is not focused, summonr rings the bell and sends a desktop notification (OSC 9) naming the workspace. Set `notify` in `config.toml`, or the Notify row in `/settings`:

| `notify` | Notifies when |
| --- | --- |
| `off` | Never |
| `turn` (default) | A turn ends |
| `all` | A turn ends or an approval is waiting |

## Passwords

When a command asks for a password through `SSH_ASKPASS` or `SUDO_ASKPASS`, such as an ssh key passphrase, a **Credential required** dialog opens. Type the password and press `enter` to send it, or press `esc` to cancel. Closing the dialog also cancels the request. The password goes to the waiting command over a private socket, never to the model, and later command output shows it as `[secret]`.

## Theme

`theme` in `config.toml` selects a preset. The Theme row in `/settings` cycles them.

| `theme` | Look |
| --- | --- |
| `space` (default) | The default preset |
| `nord` | The Nord palette |
| `dawn` | Low-glare light |
| `auto` | `space` or `dawn`, from the terminal's background |

`background = "terminal"` leaves the background unpainted so the terminal's own background (and any transparency) shows through. `"theme"`, the default, paints the theme's `base` color. The Background row in `/settings` switches between them.

Every other color is a named token you can override in a theme file. Copy [`theme.example.toml`](../../theme.example.toml) to `~/.summonr/theme.toml`, or to `<workspace>/.summonr/theme.toml` for one project:

```toml
accent = "#698cd2"
background = "#0c0c14"
code_bg = "#1e202a"
```

Both files apply on top of the chosen theme, and unset tokens keep the preset. Values are `#rgb`, `#rrggbb`, or `#rrggbbaa`. A file that does not parse, an unknown token, or a bad color is reported, and that token keeps the preset.

The tokens are `base`/`background`, `text`/`foreground`, `gray`, `dim`, `thought`, `accent`, `green`, `red`, `error`, `yellow`, `cyan`, `black`, `user_bg`, `tool_bg`, `code_bg`, `diff_add_bg`, `diff_del_bg`, `code_comment`, `code_string`, and `code_keyword`.

## Desktop window

`summonr --gui` opens the session in a desktop window, in a build with `--features gui`. The window runs the same app and layouts as the terminal.

The left rail starts with a row of buttons for the Files, Changes, Worktrees, Jobs, Peers, and Ledger panes, then lists this workspace's sessions. `+` starts a new session, and the branch button starts one in its own git worktree; merge or discard it from the Worktrees pane. Right-click a session for Rename and, on a past session, Delete.

A live session is named by `/name`, otherwise by the first line of its first prompt. Below 900 px the rail shows icons only.

The rail's footer names the provider and opens the provider list; with none set up it reads Add a provider. With no provider set up, sending opens the provider list and keeps the draft.

The main column holds the transcript and the composer card. The composer shows the `@` mentions in the draft as removable chips, an Attach button, the model, effort, and approval chips, and Send, which waits for text or an image. The title bar shows `Changes +N −M` while the workspace has changes, and `···` opens a menu with Hotkeys, Help, and Settings. Clicking outside a dialog closes it.

The Changes pane has Accept, Revert, and Note buttons on each hunk, a Revert file button and review counts on each file, and Undo after a revert; an undone hunk is not reported to the model.

Revert file and the Worktrees pane's Discard ask for a second click before they act. The Worktrees pane also has a Merge button. The This session button lists just the files this session's turns changed, from the checkpoint taken before its first turn, so changes already in the tree stay out. `c` copies the patch to the system clipboard.

| Key | In the window |
| --- | --- |
| `tab`, `shift+tab` | Move focus (in the terminal they change effort and approval) |
| `cmd+k` | Command list, with one line of help per built-in command |
| `cmd+p` | Start an `@` file mention |
| `cmd+,` | Settings |
| `cmd+n` | New session |
| `cmd+1` to `cmd+6` | Files, Changes, Worktrees, Jobs, Peers, Ledger |

Letters typed while a button has focus go to the composer. A pane takes the keyboard only after a click inside it.

## Web search and the browser

The `web` and `browser` tools are off by default. Turn them on in the Modules rows of `/settings`, or in `~/.summonr/config.toml`:

```toml
[modules]
search = true
browser = true
```

With `search` on, `web` with `query` searches DuckDuckGo, then Brave when DuckDuckGo returns nothing, with no API key. `web` with `url` fetches one page as markdown, text, or html. `freshness` takes `day`, `week`, or `month`, and `[search]` takes `count`, `allowed_domains`, and `excluded_domains`.

With `browser` on, the `browser` tool drives the Chrome, Edge, or Brave you have installed. It uses its own profile at `~/.summonr/browser/profiles/NAME` (`default` unless `[browser] profile` names another), never your everyday browser. `login` opens a visible window where you sign in. Cookies are saved after every call, so the profile stays signed in across sessions.

Each `browser` call is one action:

- A Charm action as JSON: `navigate`, `click`, `fill`, `press`, `select`, `check`, and the rest of Charm's set. Targets are `eN` refs from a snapshot, or CSS selectors.
- `snapshot`, `read`, `screenshot`, `tabs`, `login`, or `close`.

Every URL the browser opens passes the same check as `web`: no private hosts and no plain `http`. When `web` with `url` meets a page that needs JavaScript, it renders the page in this browser. `[browser]` also takes `headed`, `chrome` (the executable), and `events` (`page` and `runtime`).

## Herdr

When summonr runs inside a [Herdr](https://herdr.dev) pane, it reports lifecycle state through Herdr's CLI (`pane report-agent`): `idle` is the prompt and `working` is a turn. The summonr session id is the native session reference, and `/name TEXT` becomes the pane title and `$summary` token. Child sessions do not report, so a nested `task` does not take over the pane. Herdr has no built-in `summonr` kind; this reporter is the integration.
