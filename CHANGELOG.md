# Changelog

## Unreleased

First public release.

### Added

- Several sessions stay live in one window or terminal. `/new` starts a
  session beside the current one, and `/resume` or a click in the
  window's sidebar switches without restarting. The sidebar groups open
  sessions by project and marks each as working, needs you, ready, or
  failed. A session open in one summonr cannot be opened in another.
- The window draws its own conversation view: tool calls fold into a
  steps card, code blocks carry a copy button, and the composer holds
  the attach, model, and mode controls.
- `summonr login openai` offers Sign in with ChatGPT (Plus or Pro) next
  to the API key, and `summonr login grok` offers a SuperGrok or X
  Premium+ sign-in next to its key. A stored sign-in wins over a key;
  `summonr logout NAME` returns the row to its key. `openai` now speaks
  the Responses API. A browser login keeps the client id a provider
  issues on the redirect and accepts a `redirect_uri` with port `0`.
- `/login` groups providers as `ready`, `needs attention`, and `not set
  up`, and on a wide terminal shows a detail pane with the row's key,
  sign-in, models, and actions. `alt+k` pastes a key on a row that also
  signs in.
- The `claude-pro` seat is off. Anthropic's terms do not allow a
  third-party app to sign in with a Claude Pro or Max plan.
- Windows x86_64 release binary and a PowerShell installer
  (`irm https://summonr.dev/install.ps1 | iex`). Linux binaries are
  static musl builds. `https://summonr.dev/install.sh` installs on macOS
  and Linux.
- Every release publishes `SHA256SUMS` and build attestations
  (`gh attestation verify <file> --repo manaforged/summonr`).
- `/mcp` shows each MCP server's state, tools, schema size, and last
  error. `summonr mcp list|get|add|remove` does the same from the shell.
- MCP 2025-11-25: summonr answers `roots/list`, shows tool progress,
  fetches changed tool, prompt, and resource lists, logs server messages
  to `~/.summonr/logs/mcp.log`, and cancels a call when a turn stops.
  Servers on an unknown protocol version are refused.
- MCP prompts, resources, and forms: `/server:prompt ARGS` runs a server
  prompt and `@server:uri` attaches a resource. A server can ask you to
  fill a form; each field is checked against its schema.
- A server with more than 8 KB of tool schemas is listed under one `mcp`
  tool, so large servers do not fill the prompt.
- Plugins: `summonr install PATH` installs a plugin in the Claude Code
  layout (skills, MCP servers, hooks, `bin/`); `summonr uninstall NAME`
  removes it and `[plugins] off` disables plugins.
- `shift+tab` cycles reasoning effort from none to max. Each vendor and
  model sends the nearest level it accepts; `reasoning_levels` and
  `thinking_off` on an endpoint override this.
- `plan_mode = true` turns on plan mode. It is off by default.
- `/pane jobs` lists background commands with status, log, and kill.
  `/pane peers` lists live sessions you can mention with `@name`.
- The diff pane reviews one hunk at a time: accept, revert, or add a
  note for the model. Marks persist in `.summonr/review.json`.
- Inline terminal mode, grouped repeated tool calls, a transcript
  scrollbar, and images attached during a turn.

### Changed

- Sign-in tokens are stored apart from API keys, so signing in no longer overwrites a key. A sign-in made with an earlier build is not read; sign in again once.
- Every dialog, inline prompt, and key hint is built from one UI kit:
  dialogs share one frame and always end in their key hints, approval and
  form prompts look the same in the terminal and the desktop window, and
  hints show your own key bindings with one separator style.
- Commands run without a prompt, sandboxed or not. Only irreversible
  destruction asks first: wiping the system, home, or project root,
  `mkfs`, `dd` to a device, `terraform destroy`, cluster deletes, and
  dropped tables, including inside a heredoc fed to a shell. `sudo`,
  force pushes, pipes into a shell, git discards, and protected-path
  writes no longer ask; `confirm = "risky"` in `policy.toml` brings
  those prompts back. `--strict` refuses the whole list, and your own
  `[policy] deny` rules stay hard refusals under `--sandbox` or `--strict`.
- summonr starts when its home is not writable, such as inside another
  tool's sandbox: the session lives in the temp directory, says so,
  and does not resume. A keychain key that cannot move to the key file
  still works.
- macOS release binaries are Apple silicon only. The TLS stack does not
  build for Intel Macs, so `install.sh` stops there with a clear message
  instead of a failed download.
- Tool calls take one line each: `● Read path 1-40`, `● Run command`
  with its output under `⎿`, and `● Explored 3 files, 1 search` for a
  run of reads. A failed call turns the dot red and names its exit
  code. An edit shows its diff right under the call, split or unified.
- Research children run shell commands in a read-only sandbox, so they
  can run checks and scripts instead of replicating them with reads;
  `read` with `op=search` takes `paths` to search several paths.
- Child tasks: children the turn waits on show inline under their task
  call, one line each with their latest action, and fold to their result
  when done; background children run in a strip above the composer and
  report in the transcript when they finish. `ctrl+a` lists every child
  with its id, access, time, and tokens.
- During a turn, `enter` queues a message and `enter` on an empty
  composer sends the queue into the running turn. Command suggestions
  work during a turn; settings and pages open at once, and other
  commands run when the turn ends.
- `notify = "off" | "turn" | "all"` (the Notify row in `/settings`):
  desktop notifications when a turn ends and, with `all`, when an
  approval waits, only while the terminal is not focused.
- Three themes: `space`, `nord`, and `dawn`, a low-glare light theme;
  `theme = "auto"` picks `space` or `dawn` from the terminal background.
  Configs that name a retired theme still load.
- `ctrl+n` folds every thinking block, then every tool output, then
  shows them all again; `tools = "hidden"` keeps only the call lines.
- The footer shows the session's spend beside the context and cache
  meters when the model has a price.
- `background = "terminal"` in `config.toml`, or the Background row in
  `/settings`, uses the terminal's own background instead of the theme's.
- `/usage` is a full-screen page: tokens, cache hit, sessions, and cost
  for 24h, 7d, 30d, or all time, a daily chart, and breakdowns by model,
  workspace, and hour of day.
- The `mcp` module is on by default. A server starts on its first tool
  call and stops after two idle minutes, so configured servers use no
  memory until called. `tools`, `idle_secs`, `keep_alive`, and
  `timeout_secs` on `[mcp.NAME]` tune a server.
- MCP calls follow `policy.toml` and `--strict`, and stdio servers run
  inside `--sandbox`. A reply over 4 MiB is dropped; output over 50 KB
  is cut and the full text saved to a file.
- One permission mode: everything runs without a prompt.
  `/permissions approve` is removed.
- `sudo` asks for the password in a dialog through `SUDO_ASKPASS`.
- Fewer wasted rounds: command output keeps about 16 KB, `write` over a
  file returns one line, `read` pages by 50 KB, and batch edits to the
  same line apply in order.
- A foreground command that reaches the wait limit continues as a
  background job unless `timeout_secs` was set.
- `todo` is advertised only in steps and goal runs, `task` only in
  interactive sessions, and the `search` module is off by default.
- The last seat you used wins over `default` in `config.toml`. Effort is
  remembered per seat; a reasoning model with no effort set uses `high`.
- DeepSeek and z.ai GLM receive earlier reasoning on every turn, as both
  vendors require.
- Lower startup time, memory, and per-turn overhead: model facts come
  from the local catalog, requests are encoded once per turn, and waits
  no longer poll.

### Fixed

- Review follow-ups: `xargs -I {} sh -c {}` is seen by the ask gate; a
  failed attempt's partial reply is no longer sent back as the last
  message; the password dialog names the program that really asked;
  a hook's process group is stopped only on timeout; compaction keeps a
  later message that repeats the turn's first prompt and says when it
  starts.
- Terminal UI polish: the approval and form bands render on their own
  rows instead of over the footer; approvals list their keys
  (y allow · Y always · n deny); `?` in an empty composer opens the
  shortcuts and `↑` brings back a draft cleared with ctrl+c; counts read
  "1 hunk" and "3 hunks"; long times read 12m34s; key hints follow your
  hotkeys; pane titles ellipsize instead of hiding the close button, and a
  narrow terminal says how wide panes need; Nord and Dawn hints and code
  blocks are readable; empty states, separators, and wording match
  across panes, search, usage, and messages.
- Terminal UI: tool output can no longer move the cursor or ring the
  bell; typing y or n while an approval waits goes into the draft;
  ctrl+a and ctrl+x select and cut in a non-empty composer on Linux and
  Windows, and ctrl+u, ctrl+k, and ctrl+w can no longer be bound as
  hotkeys; child lines render the same in the strip and the transcript.
- Tools: a pre-tool hook that fails or times out blocks the call and its
  helpers are stopped; tags on lines longer than 8 KB match their edits;
  MCP over HTTP delivers progress as it arrives, refuses an event over
  the frame cap, and cancels a stopped call on the server.
- Providers: a long tool-argument stream no longer trips the stall
  timeout; a rejected OAuth token is refreshed instead of reused; OAuth
  errors never echo the token reply; cached tokens without a listed
  rate are priced at the input rate; prompt-cache writes are priced at
  the model's cache-write rate (Anthropic bills them above input); an environment key is not sent to
  a base URL you overrode; Claude 3 models other than 3.7 get no
  thinking budget.
- Sessions: auto-compaction no longer keeps the current prompt twice;
  a failed model round with partial text keeps history in step for later
  repairs; a refused dangerous command asks again next time; cancelling a
  turn stops its background jobs; a session with one bad log line still
  resumes; an aborted child reports its real reason.
- Sandbox and policy: `--strict` refuses an allow-listed command that
  destroys work, such as `git reset --hard`; git hooks, config, and
  commondir stay protected inside submodules too; the ask gate sees
  quoted verbs, `eval`, `find -exec rm`, `bash -c --`, and `xargs sh -c`;
  a password prompt names the program that asked for it.
- MCP: a timed-out call no longer corrupts the next one, server start
  waits at most 60 seconds, remote servers reconnect after their session
  expires, and Ctrl-C stops servers and commands.
- Providers dialog: probe results regroup the list and fill each row's models without opening Models first; Enter works on a keyless row, and a local row such as `ollama` reads "none needed" for its key instead of asking for one; a login or logout on the active seat applies at once; a key pasted on `openai` is checked against the API, not the ChatGPT sign-in; a stream failure keeps its error message.
- A stream that sends only keep-alives fails after the idle timeout and
  is retried, instead of holding the turn.
- A hook that writes more than 64 KB no longer stalls the tool call.
- Leaving plan mode removes the sandbox plan mode turned on.
- A failed compaction summary retries after 8 rounds.
- A file whose change time moved is hashed again for undo snapshots.

### Security

- A plan sign-in must return the nonce summonr sent and grant plan access, and a redirect that names a different client than the one on file is refused.
- A workspace `SYSTEM.md` loads only when the workspace is trusted.
- `~/.summonr` is created with mode `0700`, and existing directories are
  tightened to it.
- A non-empty `policy.toml` in an uncontained session warns at startup
  that its rules are not enforced.
- `Endpoint` debug output shows only whether a key or header is set,
  never its value.

## 0.1.0-rc.1 - 2026-09-23

### Added

- Release binaries for macOS (arm64, x86_64) and Linux (x86_64, arm64),
  each with a SHA-256 file, and `scripts/install.sh` to download,
  check, and install one.
- The `/model` picker opens from cache in about 0.3 s and checks each
  provider in the background. It groups current, pinned (`alt+p`), recent,
  and providers, and shows one state word per provider with a one-line
  fix. `F5` rechecks a failed provider. `summonr endpoints` uses the
  same state words and the same discovery path.
- Sign-in happens inside the dialog. A pasted key is checked against the
  provider before it is saved. OAuth shows the device URL, code, and
  elapsed time. `/login` filters and groups providers, `/login NAME`
  opens the key prompt, and `alt+d` signs out.
- Endpoints read keys from OpenCode's `auth.json` and Kimi's
  `kimi-code.json`. `summonr auth` names the source.
- Built-in endpoints `together`, `groq`, `cerebras`, `mistral`, `muse`,
  `zen`, `go`, and `opencode`. `responses` is a selectable wire.
- A sandboxed write or network denial asks `[ allow ]`, `[ always ]`, or
  `[ deny ]`.
- `web` fetches as markdown, text, or html.
- A fenced `chart` block, or mermaid `xychart`, renders as labeled bars.
- `theme` accepts `space`, `fantasy`, `nord`, `gruvbox`, `solarized`,
  and `light`.
- CLI verbs `summonr resume`, `summonr install`, and
  `summonr doctor --logs`.

### Changed

- Summonr depends on the one Sigil package `sigil-ui` (feature
  `terminal`) instead of `sigil-gui` plus `sigil-tui`.
- The terminal interface runs on Sigil's terminal loop. Sigil restores
  the terminal on exit, panic, and SIGTERM, SIGHUP, or SIGINT.
- The `/` popup and `/help` list 30 commands, grouped by job. Old names
  still work for one release; see Renamed commands.
- `ctrl+d` only quits. `alt+d` deletes a session in the sessions
  picker, signs out in `/login`, and sets the default in `/model`.
  `alt+p` pins a model. `F5` is the only picker refresh; `ctrl+r`
  stays prompt history.
- `↑` on an empty draft pulls queued input back; `ctrl+q` no longer
  does.
- A key stored with `summonr login` wins over peer keys. An endpoint
  with no key fails with the missing-key message.
- The model sees five core tools: `read`, `edit`, `write`,
  `run_command`, and `todo`. Modules add `skill`, `lsp`, `web`, `task`,
  and `bus`. `read` search takes `literal`, `caseSensitive`, and
  `include`; `edit` and `write` take a `files` batch.
- Chat runs at once. `shift+tab` and `/plan` enter plan mode, which is
  read-only and saves the plan to `.summonr/plan.md`. `/plan accept`
  runs the plan in the same turn.
- The `task` module is off by default.
- `--goal`, `--steps`, and `implement` children edit the launch
  directory. `--worktree` isolates the session.
- Sandboxed network is on by default; `--no-net` denies it.
- Anthropic: models before Claude 4.6 get a manual thinking budget,
  newer models get adaptive thinking. Defaults are `claude-sonnet-5`
  and `claude-opus-5-5`.
- Gemini tool schemas use `parametersJsonSchema`. Effort maps to
  `thinkingBudget` on 2.5 and `thinkingLevel` on 3.
- Dialogs size from the terminal and repaint on resize. Copy is
  `ctrl+b` everywhere.
- A malformed `config.toml` stops startup with one error.
- `src/limits.rs` holds every timeout. Transport retries run in
  leyline's `RetryPolicy`. A stream cut mid-body retries once.
- The per-turn git snapshot runs in the background. The transcript
  repaints only changed entries, and an idle session skips paints.
- Multi-path `read` shares one 50 KB cap. The tool schema is frozen for
  the session.
- Release builds use fat LTO, one codegen unit, and strip.
- Crate and command are `summonr`. `summon` and `summoner` stay PATH
  aliases.

### Fixed

- Anthropic tool results carry `is_error` and images. Usage counts cache
  reads and the final `message_delta`. Refusals show the provider text.
- Gemini keeps text-part signatures, reports blocked prompts, sends the
  real function name, and honors `retryDelay` on 429.
- Responses shows stream errors, streams commentary and reasoning
  summaries as reasoning, and keeps a `max_output_tokens` stop as a
  length stop. Resumed sessions keep message phase.
- Error frames and cut streams fail instead of returning a partial
  reply. Thinking replays only to the model that produced it.
- `--json` keeps its event order, and an abort emits Aborted then
  Receipt.
- Compaction and tool-argument repair are saved in the journal, and
  compaction resets read dedup.
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

### Earlier work in this release

- Native Anthropic and Gemini requests include user images. Gemini tool
  calls retain thought signatures. Anthropic tool turns retain ordered
  content, including signed and redacted thinking, across streamed responses
  and session replay. Continuation data stays scoped to its wire and model.
- Repository policy intersects user allow and write rules. Children inherit
  both sets of restrictions.
- Command policy splits on `|` and `;` outside quotes, so a grep pattern
  such as `pwsh\|sh` is not a pipe into a shell. A `cd` to
  `$(git rev-parse --show-toplevel)` counts as the workspace.
- `[sandbox] write` in `~/.summonr/config.toml` lists directories outside the
  workspace that contained commands may write, for build caches that
  otherwise fail with `Operation not permitted`.
- A research child that finishes before its batch is cancelled keeps its
  result; the model reads it on the next turn.
- Fenced code in replies colours keywords; `/export [PATH]` writes the
  session as markdown from inside the interface, also during a turn.
- Pictures in the transcript: a pasted or attached image, a `show_image`
  tool call, or a markdown image link in a reply renders through the
  terminal's graphics protocol (Kitty or iTerm2) and as half-block cells
  elsewhere. The drawing lives in Sigil's terminal host.
- A fenced `mermaid` block in a reply renders as a diagram: flowcharts,
  state diagrams, sequence diagrams, and pie charts, through Sigil's
  `mermaid` widget.
- The terminal no longer reports mouse motion without a button, so moving the
  mouse over an idle session stops pinning a CPU core and no longer starves
  selection and scroll.
- Plan mode writes files. The write_file tool writes any path so a plan or
  review lands; the edit tools and shell commands stay read-only, so source
  edits and builds are refused, and a blocked write says why.
- A picture scrolled partly off the screen shows its visible rows;
  `read_file` on an image reports its size and shows it; `/view` and the
  tree open image files as pictures in the view pane.
- Tool cards drop a leading `cd … &&`, and the working status sits on its
  own row above the composer with the output rate in tokens per second:
  a three-second estimate while text streams, the provider's count over
  the streamed span once the round reports usage.
- Workflow changes resolve every seat before activation. A failed seat
  leaves the previous workflow and session log unchanged.
- Experimental Windows command execution uses PowerShell, AppContainer,
  and Job Objects. It builds only with the `windows-sandbox` cargo
  feature; without it, `--sandbox` on Windows fails with an error. Cancellation stops descendants and removes temporary
  workspace permissions. Git for Windows and user-installed toolchains
  remain blocked in the default sandbox.
- Linux command cancellation now kills the process group, including
  descendants that outlive their parent command.
- Language-server file URIs encode Windows paths and special characters.
- New session IDs preserve creation order within a process. Background
  verification uses the latest completed attempt of a repeated command,
  while unfinished attempts still prevent completion.
- The tool schema sent every round is smaller. Descriptions are one or
  two sentences. `diagnostics` and `definition` appear only when a
  language server for the workspace is on PATH or configured; `skill`
  appears only when a skill catalog exists. Every tool still runs when
  called.
- Herdr panes show the Summoner session name and model, and ordered
  lifecycle reports. A live fork retargets the pane to the new session
  id.
- `task` children keep the parent's shell and language-server tools,
  share the parent's remaining token and time budget, and stream every
  tool call, result, and answer into the parent transcript under the child
  id.
  Cancelling a batch removes child worktrees that hold no change.
  `task` also takes `background`, `child`, and `kill`: start a child and
  return its id, poll its state and last action, or stop it. A turn defers
  while a background child runs.
- Work remembers deferred background jobs by id across turns; a later
  turn cannot complete while one still runs. Every goal turn restates the
  objective. Goal-judge repairs and provider retries (exponential backoff)
  have fixed caps; provider rounds have none. Receipts carry
  `verified`. Changing `.summonr/check.toml` mid-turn re-selects the checks
  instead of blocking the session.
- Context overflow recovery clears the oldest tool results first and keeps
  the newest two. A failed auto-compaction is not retried until the next
  turn.
- `summon room`: Claude Code and Codex join the bus through
  `summon room serve`, a stdio MCP server with join, agents, send, inbox,
  wait, board, status, and leave. `summon room host codex` runs Codex as a
  peer that answers mail on one thread. `summon room setup claude|codex`
  installs the `/summon-room` skill and the MCP entry. Roster records and
  mail carry the agent kind.
- Command is `summon` or `summoner` (same binary). Crate is `summoner`.
- Connector owns OpenAI `chat/completions` (HTTP, SSE) and native
  Anthropic Messages / Gemini wires. Credentials: API key, RFC 6749
  token, or RFC 8628 device login. CoT is `reasoning_content`. The
  `task` tool runs depth-one child sessions concurrently. Writable
  children use separate Git worktrees. Summoner combines their patches and
  removes the worktrees before the parent verifies once. Child sessions have
  no web or background tools. `/model` lists every resolvable seat.
- Work: plan, execute, verify. `--steps` and `--goal` hold the job.
- Git worktrees for `--goal` / `--steps` when the directory is a repo.
  Launch does not sweep live trees. `prune` only forgets missing dirs.
- TUI (Sigil), policy, sandbox, session journal.
- Commands and post-edit checks run in the OS sandbox with network denied
  unless the user passes `--net` or `--no-sandbox`.
- Turn receipts return every completion check separately from model text.
  Diagnostics use one locked JSONL file per run; `logs` lists them and
  `doctor` reports crashes or incomplete runs. Malformed goal verdicts
  stop the Work instead of silently continuing it.

Not in this release: crates.io (Sigil is a git dependency) and renaming
`~/.summonr`. Windows AppContainer is unfinished: Git for Windows and
user-installed toolchains stay blocked.
