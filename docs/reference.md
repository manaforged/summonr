# Summoner reference

This page documents every flag, key, slash command, pane, policy rule,
workflow seat, child session, and the agent room. The
[README](../README.md) is the short form.


## Usage

```sh
summon grok
summon anthropic
summon kimi -p "What is in Cargo.toml?"
```

Add any other host in `~/.mage/config.toml`:

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8081/v1"
model = "whatever-the-server-calls-it"
api_key = "local"
```

Then `summon local`. `effort = "low"` on a row (or `--effort LEVEL`,
`/effort LEVEL` in the interface) sets the reasoning effort for that
seat. Set it only on a seat whose model reasons; a host that does not
accept `reasoning_effort` rejects the request. `price = { input, output, cached }` in USD per million tokens
prices every round in `/stats` and the footer. A workflow seat keeps
its own row's effort and price; `--effort` and `/effort` set the
driving seat and any role seat whose row does not say.
`summon endpoints` probes `GET /models` for each name so a dead host fails before the first tool round. `summon login NAME`
prompts for an API key, or runs RFC 8628 device-code login when that row
has `device_url`.

Keys live in `~/.mage/credentials.toml`, a sealed ChaCha20-Poly1305
blob (mode 0600, written through a rename). The wrapping key is a
32-byte item in the OS store (`summoner` / `seal`). `cat` of the file
shows ciphertext. File mode also needs that keychain item: if the
keychain is locked, Summoner reports an error and keeps `config.toml`
untouched. The key prompt disables echo and stores the key;
`summon login NAME --key` forces it for a device-login endpoint.
`summon logout NAME` forgets the key and any login
token. A plaintext `api_key` in `config.toml` is moved into that file
the first time Summoner runs, and it says so once. Resolution order is
the host's environment variable, the store, then `api_key` in
`config.toml`.

If you want the OS store instead, set `secrets = "keychain"` in
`config.toml` (or `MAGE_SECRETS=keychain`). On macOS that is one
Keychain entry holding every secret, read and written through
`/usr/bin/security`, so the one-time "Always Allow" is granted to
Apple's signed tool and survives every rebuild of Summoner; Windows uses
Credential Manager and Linux the Secret Service, one entry per secret.
A locked keychain is reported, never worked around with a file, and
the two modes never fall back to each other.

Built-ins: chat/completions seats `zai`, `zai-cn`, `glm`, `glm-intl`,
`qwen`, `kimi`, `grok`, `copilot`, `openai`, `openrouter`; native wires `anthropic`
(Messages) and `gemini`. `summon login copilot` is GitHub device login;
chat then exchanges that token for a Copilot session token. Kimi is a
key (`KIMI_CODING_API_KEY`, `KIMI_API_KEY`, or `MOONSHOT_API_KEY`) or a
token in `~/.kimi-code/credentials/kimi-code.json`. Environment keys are the host's own
(`XAI_API_KEY`, `ZAI_*`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`,
`ANTHROPIC_API_KEY`, `GEMINI_API_KEY`). Overlay `wire = "anthropic"`
on any `[endpoint.NAME]` row. See [`config.example.toml`](../config.example.toml).

```sh
summon --goal "make cargo test pass"
summon --steps PLAN.md
```

`summon --name TEXT` (or `/name TEXT`) names a session; `--session`
takes a name or an id prefix, `summon sessions` lists both, and
`summon export [ID|NAME]` prints the transcript as Markdown.
`summon logs` lists the last 20 per-run diagnostic logs. `summon doctor`
prints the running binary path and reports crashed or incomplete runs.
Logs older than 14 days are removed unless their process is still active.

`--goal` keeps one objective until `.mage/check.toml` is green and a
judge says stop. The judge answers `done`, `continue`, `stalled`, or
`impossible`; the last two end the goal with the reason. A goal has no
built-in turn cap. `--max-turns N` bounds it: a goal that is still red
or still `continue` at turn N stops, exit 2 when the checks were red
and exit 3 when the judge said continue. `--max-tokens` and
`--max-secs` bound every mode. Every goal turn restates the objective
to the model, so a compacted context cannot lose it. `--steps` hands
the model one numbered item at a time. A turn that ends while a
background command it started is still running is deferred, with no
verdict and no repair strike, until the job exits or the model kills
it. The harness remembers that job across turns; a later turn cannot
complete while it runs. A `### Heading` under
`## Steps` groups the items that follow it into a phase. Every turn
carries the ledger to the model and to the transcript: phases as Roman
numerals, `[x]` done, `[/]` this step, `[ ]` later, `[!]` blocked.
Both isolate in a git worktree (`<repo>-worktrees/<id>`) when you are
in a repo. Exit 0 commits the files the run changed as
`summon: <objective>` (files over 5 MB and anything the run did not
touch are left in place and named), fast-forwards your branch, and
removes the tree; a tree that cannot fast-forward or still holds
uncommitted files is kept and named, and the exit code is 1.
`--here` stays put.

Auto mode follows the request. Reviews, explanations, and diagnoses inspect
the workspace without changing it. Change requests can edit and run checks.

Plan mode adds a read-only workspace boundary. The model can run inspection
commands, but commands cannot change workspace or repository state. Toggle plan
`/plan accept`
keeps the journaled plan and returns to auto; your next prompt runs
inside that Work, and the transcript and the model both see
`plan accepted`. `/plan reject` leaves plan mode without implementing.
`/plan` and `/auto` only switch the tool set. `diagnostics` and `definition` route each file to its
language module (`definition` also lists references and applies rename).

### Workflows

A workflow seats each step on its own model. Roles are `scout`, `plan`,
`implement`, `review`, `judge`, and `summary`; a role the workflow does
not name runs on the seat you started with.

```toml
# ~/.mage/config.toml
workflow = "cheap-scout"      # optional default

[workflows.cheap-scout]
plan = "grok"
review = "zai"
judge = "grok"
```

`summon -w cheap-scout`, `/workflow cheap-scout` in the interface, or the
`workflow` key picks one; `summon workflows` lists them. In the
interface every card action has a verb too:

```
/workflow                    the card
/workflow NAME | off         apply a saved workflow, or none
/workflow list | show        saved workflows · the live seats
/workflow set ROLE ENDPOINT [MODEL]
/workflow clear ROLE
/workflow save NAME          write the live seats as [workflows.NAME]
/workflow default NAME       the workflow new sessions start with
```

## Interface

`summon --gui` opens the session in a desktop window (a build with
`--features gui`). The window is the Codex shape on Sigil: a left rail
that switches the Files, Changes, Worktrees, and Plan panes and lists this
workspace's sessions; a main column with the transcript, a composer card,
and an Auto/Plan switch; a right rail that shows the open pane with real
widgets: clickable file rows, +/− chips that open a file's hunks, an
Edit/Save file view. The title bar's changes badge is live. The same
`App`, keys, and layouts drive the terminal and the window.

The screen is a set of panes on one session. `ctrl+e` (or `/pane
files`) opens the repository tree beside the chat; `ctrl+l` the plan
ledger; `ctrl+p` hides or restores the pane. In the tree: `ctrl+→`
focuses it, `↑↓` move, `←→` collapse or expand, `PgUp/PgDn/Home/End`
jump, Enter opens a directory or attaches `@path` to your prompt, and
a right-click (or the mouse) opens a menu: attach, open in `$EDITOR`,
insert path. `ctrl+←` or `esc` returns to the composer. `/pane off`
closes the pane. `/layout focus`, `run`, `files`, and `review` are
presets, `/layout "chat | files 30%"` sets the split, and
ctrl+shift+←/→ resizes it. The layout persists in `~/.mage/config.toml`
as `layout`. `ctrl+o` opens the settings modal, where every hotkey is
editable; the choice is stored under `[hotkeys]` in the same file.

`ctrl+x` (or `/pane diff`, `/diff`) shows the changes against the base:
the worktree base for a `--goal`/`--steps` run, `HEAD` otherwise. Each
row is `+added −removed path`; Enter or `→` opens that file's hunks,
`↑↓` scroll them, `←` or `esc` returns to the list, and the right-click
menu adds view hunks. `/diff PATH` jumps to one file. After a turn that
changed the tree, the transcript gets one `changes: N files · +a −r · vs BASE ·
/pane diff` line. The same data is `summon diff [PATH]` (the patch), `summon diff
--stat`, and `summon diff --json`.

`→` on a file in the tree (or `/view PATH`, or the menu's view file)
reads it in the view pane with line numbers; lines the diff added are
green with a `+`. `↑↓`, `PgUp/PgDn`, `Home/End` scroll, `o` opens it in
`$EDITOR`, `esc` returns to the tree. `e` edits the file in place: the
keyboard goes to the file (type, Enter, arrows, Home/End), the caret is
the inverted cell, `ctrl+s` saves, and `esc` leaves. With unsaved text a
second `esc` discards it. The tree, the diff, and the view
follow the file system: a change on disk relists, re-diffs, or reloads
the pane within about half a second.

In the diff pane `t` keeps only the files the last turn edited (the
header says `· last turn`), `c` copies the patch on show to the clipboard
(OSC 52 in the terminal; the system clipboard in the window), and a pane
80 columns or wider shows a file's hunks side by side, old on the left.
`summon diff --turn N` (or `--turn last`) is the same filter for the
latest session in this directory.

While a turn runs, a row above the composer shows a spinner, what the
model is doing (`thinking`, or the tool it is running), the round, the
seconds so far, the output rate, and the queue count; the composer's bottom
border keeps the keys: `esc` cancels, `enter` steers, `tab` queues.

The output rate is tokens per second. While text streams it is an
estimate: characters received in the last three seconds, divided by the
characters per token measured on the previous round (4 until then), over
that span. When the provider reports the round's usage, the rate becomes
exact: output tokens over the time between the first and last streamed
chunk, or over the whole round when nothing streamed, as in a round that
only called tools. The footer shows the last round's rate after the turn.

### Keys

| Key | Idle composer | During a turn |
| --- | --- | --- |
| `enter` | send | steer: delivered after the current tool calls |
| `tab` | complete a `/command` or `@path` | queue: delivered after the turn |
| `alt+↑` | | pull queued input back into the editor |
| `esc` | dismiss; twice within two seconds opens `/tree` | cancel the turn |
| `ctrl+c` | clear the draft; on an empty draft, twice within two seconds quits | cancel the turn |
| `ctrl+d` | quit when the draft is empty; in the sessions picker, twice deletes the selected session | |
| `ctrl+j` | newline (default; editable in options) | |
| `ctrl+g` | edit the draft in `$EDITOR` (default) | |
| `shift+tab` | switch between auto and plan | takes effect when the turn ends |
| `alt+←` `alt+→` `alt+b` `alt+f` | word left or right | |
| `ctrl+w` `alt+backspace` | delete the word before the caret | |
| `alt+d` | delete the word after the caret | |
| `ctrl+u` `ctrl+k` | delete to the line start or end | |

A draft that starts with `!` runs in the command sandbox before any
model round: `!cmd` shows the output as a tool card and then sends
`$ cmd` plus the output as the prompt; `!!cmd` shows the output only.

A paste of eight or more lines, or a thousand or more bytes, folds into
`[pasted N lines #k]` in the draft. Summoner expands every token back to
the pasted text when the prompt is sent or queued; `ctrl+c` discards
them with the draft.

While a turn runs, the composer status also counts waiting input
(`2 queued, alt+↑ edits`). The terminal title reads
`summon · <workspace> · working` or `· ready`, and the terminal bell
rings once when a turn ends while the terminal is not focused.

`/tools` switches tool output between compact and full; `/reasoning`
shows or hides streamed reasoning; `/model next` and `/model prev`
step through the ready seats without the picker.

`/export` writes the session as markdown to `~/.mage/exports/<id>.md`;
`/export PATH` writes it where you say. It works while a turn runs.
`ctrl+y` copies the latest assistant reply. `/copy all` copies the full
transcript. Both use OSC 52 in the terminal and the system clipboard in the
window. `ctrl+t` releases the mouse to the terminal so you can select and copy
any transcript span; press it again to restore mouse interaction.

`/pane worktrees` lists the `summon/*` worktrees of this repository:
branch, commits ahead of the base, dirty or clean, the session that ran
there, and the directory. Enter or `→` opens that worktree's diff (`esc`
comes back); the right-click menu offers show diff, merge (the same
ff-only merge a run ends with; a dirty tree is refused), remove
worktree (only when clean and fully merged), and insert path. `summon
worktrees` prints the same rows; `summon worktrees --json` as JSON.

The card takes the keyboard (↑↓ role, enter picks a seat, `d` returns
it to the driving seat, ←→ cycles saved workflows, `s` saves) and the
mouse: click a role to pick its seat, drag a role onto another to copy
the seat, click a saved name to apply it. The settings modal, model
picker, and authentication card take clicks as well. The session log
records the model behind every reply.

With a `scout` seat, a `--steps`, `--goal`, or plan-mode job opens with
a read-only brief of the files, conventions, and helpers the objective
touches. With a `review` seat, a turn whose checks passed is read by
the reviewer before it completes; a rejection sends the findings back
as the next instruction and counts as a failed check against
`--max-turns`.
## Child sessions

The `task` tool delegates work to fresh child sessions. A model can emit
several `task` calls in one response. Summoner runs them concurrently.
Each `implement` child edits a separate Git worktree from the same tracked
`HEAD`; the tracked files must be clean. Implement children get the parent's file, shell, and language-server
tools inside that worktree; background commands stay with the parent.
Summoner applies every child patch in one child worktree first. A conflict
keeps the child worktrees and leaves the parent unchanged. After a clean
apply, Summoner removes the child worktrees. The parent verifies once and
commits. Outside Git, only one implement child can write the current
directory at a time.

A `research` child reads the parent workspace without a worktree. It
cannot write files or start background commands; shell commands run only
inside the sandbox. Each child uses the endpoint context window. If the
parent has a token or time budget, the children share what remains of it;
otherwise a child runs until it finishes or you cancel it. Children have no web tools or another `task` tool. Every child
tool call, result, and answer streams into the parent transcript under the
child id, and Esc during a batch removes child worktrees that hold no
change. A research child that finished before you pressed Esc keeps its
result: the model reads it on the next turn instead of an `aborted` answer.

`task` with `background: true` starts the child and returns its id at
once. `task` with `child: ID` reports its state, elapsed time, and last
action while it runs, and its full result once it finishes; `kill: true`
stops it. A turn does not end while a background child runs: Summoner
waits, then defers the turn the way it does for a background command,
until the child finishes or the model kills it. Background implement
children need a Git repository.

## Modules

Every optional capability is a module compiled into the binary: `search`,
`lsp`, `jobs`, `task`, `skills`, `bus`, and `gui`. `summon modules` lists
them with their state; `/modules NAME on|off` in the interface switches one
and writes `[modules]` in `~/.mage/config.toml`. A workspace
`.mage/config.toml` may turn a module off for that project but cannot turn
one on. Summoner runs no third-party code inside the harness; modules read
declarative files under `.mage` where they need user input.

`grep` takes `glob`, honors `.gitignore`, and lists matching files
when the pattern is empty.
`web_search` reads DuckDuckGo HTML without a key (a `BRAVE_API_KEY` in the
environment selects Brave instead). `[search] backend = "bing"`
keeps the Bing RSS compatibility path; `[search] backend = "brave"` with
`api_key` (or `BRAVE_API_KEY`) uses the Brave Search API. Every result says
which backend answered. Language modules cover Rust,
TypeScript/JavaScript, Python, and Go. They select the server from the file and
a project marker, then keep one connection per module in a polyglot checkout.
Rust passes initialization options that disable check-on-save, build scripts,
proc macros, and `cargo --all-targets`. Explicit Cargo commands still run
through `run_command`.

A workspace `.mage/config.toml` may force a built-in module:

```toml
[lsp]
module = "rust"
```

Repository config cannot supply executable argv. A trusted
`~/.mage/config.toml` may use `[lsp] command = ["server", "--stdio"]` when a
built-in module does not fit.
Language servers run as trusted host processes with the inherited environment,
outside the command sandbox. They can access files and network resources with
your account's permissions. Rename edits returned to Summoner still pass through
its file policy. Disable the `lsp` module when you do not trust the server or
the project configuration it reads.

Unknown flags exit 1. A red verify gate exits 2 (`-p`, or a bounded
goal that ends red). A run that stops short exits 3: a budget breach, a
stalled or impossible goal, a bounded goal the judge did not accept,
or an aborted turn.
JSON turn receipts keep model text separate from the changed-path list
and every configured completion check. `verified` on a receipt is true
only when at least one check ran and every check passed; a turn with no
configured check completes with `verified: false`.

## Room

`summon room` puts several coding agents on this machine in one
conversation. The room is the `bus` module's files under `~/.mage/bus`: a
roster, one inbox per agent, and one blackboard per workspace. Summoner
sessions join it natively. Claude Code and Codex join through
`summon room serve`, a stdio MCP server over the same files with the
tools `join`, `agents`, `send`, `inbox`, `wait`, `board`, `status`, and
`leave`. Mail is peer text, never user instructions, and each session's
own permissions still apply.

```
summon room setup claude     # installs the /summon-room skill and the .mcp.json entry here
claude --dangerously-load-development-channels server:summon-room
/summon-room reviewer
```

The skill joins the room, starts Codex as a peer when none is present,
and lays out the split: claims on the board, one brief per peer, `wait`
only when blocked, then reconcile against the proving command.

`summon room host codex [--cwd PATH] [--id NAME] [--read-only]` runs
Codex as a peer that answers mail. It spawns `codex mcp-server` on
Codex's own login, keeps one thread, and sends each answer back to the
sender. Codex cannot receive a push into a live session, so this is how
Codex takes part without a second terminal polling `wait`.
`summon room setup codex` installs the same skill and the
`~/.codex/config.toml` entry for an interactive Codex session.

Delivery: `wait` blocks until mail arrives or the deadline passes;
`inbox` returns at once. When the client is Claude Code, the server also
pushes each mail as a `notifications/claude/channel` event. The push is a
copy, so a mail that the client did not surface is still in the inbox.
`MAGE_ROOM_PUSH=off` in the server's env block turns the push off.
`summon room` alone prints the roster.

## Limits

Each flag alone gives these modes:

| Mode | File tools | Shell commands | Shell network |
| --- | --- | --- | --- |
| Default | Reads any path the sandbox can read; writes inside the workspace under policy | Contained; workspace and temporary storage writable | Denied |
| `--plan` | Workspace reads; writes denied | Contained inspection; workspace read-only; no background jobs | Denied |
| `--no-sandbox` | Full access with your account's permissions | Uncontained; command policy bypassed | Your account's access |

`read_file`, `list_dir`, and `grep` accept an absolute path outside the
workspace, which is what the shell can already read; every edit and write
tool stays inside it. For work that spans sibling repositories, launch
Summoner in the parent directory or pass absolute paths to the read tools.

Commands use Seatbelt, bubblewrap, or experimental Windows AppContainer.
A missing backend is a hard stop. `--net` allows sockets inside the sandbox;
it does not change file permissions. Model requests and language servers use
host networking separately from shell commands.

`--no-sandbox` also bypasses file allow/write restrictions and permits file tools
outside the workspace. `--plan` still denies file writes when combined with it,
and refuses shell commands because read-only inspection requires a sandbox.
`--strict` reapplies policy restrictions but does not restore process containment.

A build tool that writes outside the workspace fails inside the sandbox with
`Operation not permitted`. Name those directories in `~/.mage/config.toml`
and every contained command, including a research child's, can write there:

```toml
[sandbox]
write = ["~/.cargo/registry", "/Volumes/build/cache"]
```

A `~/` prefix expands to your home directory. A listed path that does not
exist stops startup.

A provider reply of 429, a 5xx status, or a transport error is retried up
to six times with a wait that doubles from 2 s to 60 s. A `Retry-After`
header in seconds replaces that wait, up to 600 s. A reply that already
streamed output is not retried.

User policy is `~/.mage/policy.toml`; repository policy is `.mage/policy.toml`.
An action must match every configured nonempty allow/write list. Deny lists
accumulate. Empty lists add no restriction, except that `strict = true` with an
empty allow list denies all commands. Repository policy can narrow user
permissions, never widen them. Children inherit these restrictions.
A restricted write list makes contained shell commands read-only and disables
background commands; file tools still use the list.

- Home is still `~/.mage`. Env is still `MAGE_*`.
- An older `mage` binary may still be on `PATH`. Use `summon`.
- No MCP client for third-party servers, hook API, or runtime plugin
  loader. Language modules are compiled into the binary. A CLI with a README is a tool the
  model already knows how to run through `run_command`; a skill under
  `~/.mage/skills` or `.mage/skills` carries instructions; the bus carries
  mail between sessions.

## Herdr

When `summon` runs inside a [Herdr](https://herdr.dev) pane, it reports
lifecycle state through Herdr's CLI (`pane report-agent`). `idle` is the
prompt; `working` is a turn. The Summoner session id is the native session
reference. `/name TEXT` becomes the pane title and `$summary` token.
Child sessions do not report, so a nested `task` does not steal the pane.
Herdr has no built-in `summoner` kind. This reporter is the integration.

## Docs

- [Work](architecture.md)
- [`config.example.toml`](../config.example.toml)
- [`policy.example.toml`](../policy.example.toml)
