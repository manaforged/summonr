# Changelog

## Unreleased

### Added

- `/mcp` shows MCP servers: state, tools, schema size, and the last
  error. `summonr mcp list|get|add|remove` does the same from the shell.
- MCP 2025-11-25: summonr answers `roots/list` with the workspace, shows
  tool progress (and progress keeps a slow call alive), fetches a
  changed tool list again, logs server messages to
  `~/.summonr/logs/mcp.log`, and cancels the call when a turn stops.
  Servers on an unknown protocol version are refused.
- Plugins: `summonr install PATH` installs a plugin in the Claude Code
  layout (skills, MCP servers, hooks, `bin/`) under
  `~/.summonr/plugins/`; `summonr uninstall NAME` removes it and
  `[plugins] off` disables it. `[hooks]` values can be lists. Skill
  summaries come from the frontmatter `description`. Plugin data lives
  in `~/.summonr/plugin-data/<name>/`; `userConfig` is reported, not
  prompted.
- A server with more than 8 KB of MCP tool schemas is listed by tool
  name under one `mcp` tool; the model looks up schemas and calls tools
  through it, so large servers no longer fill the prompt.
- MCP prompts, resources, and forms: `/server:prompt ARGS` runs a
  server prompt, `@server:uri` attaches a server resource, and the
  composer suggests both. A server can ask the user to fill a form during
  a call; each field is checked against its schema and the answer is
  reviewed before it is sent. Without a user the form is declined.
  Changed prompt and resource lists are fetched again.
- `shift+tab` cycles the reasoning effort: none, minimal, low, medium,
  high, xhigh, max.
- Effort is correct on every built-in vendor and model: each preset and
  model row declares the levels it accepts and how it turns thinking off,
  and the nearest accepted level is sent. `reasoning_levels` and
  `thinking_off` on an endpoint row override them.
- A `plan mode` setting (`plan_mode = true`). It is off by default, and
  `/plan` is refused while it is off.

### Changed

- The `mcp` module is on by default. A server starts on its first tool
  call and stops after two idle minutes; its tool list comes from a
  cache in `~/.summonr/mcp/`, so configured servers cost no memory
  until used. `tools`, `idle_secs`, `keep_alive`, and `timeout_secs` on
  `[mcp.NAME]` narrow and tune a server.
- MCP calls follow `policy.toml` deny and allow lists and `--strict`,
  and stdio servers run inside `--sandbox`. A call with no reply is
  cancelled after five minutes. A reply over 4 MiB is dropped, and
  output over 50 KB is cut with the full text saved to a file. Images
  from MCP tools reach the model.
- Project `.mcp.json` servers that wait for approval are named at
  session start.
- MCP fixes: images reach only models that take them; a server restarts
  when the sandbox changes; starting a server waits at most 60 seconds;
  a timed-out call no longer corrupts the next one; a request that
  cannot be written times out; tool lists are cached per workspace and
  an empty list is fetched again; error replies are cut to 50 KB;
  Ctrl-C stops MCP servers and commands; remote servers reconnect after
  their session expires; `idle_secs` and `timeout_secs` must be at
  least 1; command allow lists no longer block MCP tools outside
  `--strict`.
- Batch edits that touch the same line apply in order instead of failing
  as "overlapping"; an edit with unrecognised keys names them; an edit's
  diff is capped at 80 rows. The edit description leads with old+new.
- DeepSeek and z.ai GLM seats send the model's earlier reasoning back
  on every turn, as both vendors require during tool-calling loops.
- The default system prompt says what done means: treat the request as
  a contract, run the result the way the request says it will be used,
  and finish only when every requirement is met and verified.
- Fewer wasted rounds. Command output keeps about 16 KB and names the
  full log only when it cut something; `write` over a file returns a
  one-line summary; `read` pages by 50 KB; only exact `cat`, `head`, and
  `sed -n` become reads, and `ls`, `wc`, and `grep` run in the shell.
- A foreground command that reaches the wait limit keeps running as a
  job instead of being killed, unless `timeout_secs` was set; a job poll
  waits for the job.
- `todo` is advertised only in steps and goal runs, `web` (the `search`
  module) is off by default, and `task` is advertised only in
  interactive sessions.
- The compaction summary request carries no tools, and a failed summary
  retries after 8 rounds instead of never in that turn. A headless reply
  with no tool call to a harness control message is re-prompted once,
  and job polls do not trip the loop guard.
- The last seat you used wins over `default` in `config.toml`; the
  default applies only when there is no last seat.
- Effort is remembered per seat. A seat with no effort uses `high` when
  the catalog says its model reasons, instead of sending nothing.
- One mode: everything runs without a prompt except hard denials
  (`sudo`, `rm -rf` outside the workspace, force-push, the deny list),
  which ask for confirmation. The reviewer model and
  `/permissions approve` are gone.
- `sudo` in a command asks for the password in the password dialog,
  through `SUDO_ASKPASS`. Before, `sudo` failed with no terminal.

- `/pane jobs` lists this workspace's background commands with a live
  status badge, a log view, and a kill action shared with
  `run_command` job control.
- `/pane peers` lists the live mail roster; Enter or the menu inserts
  an `@name` mention that delivers to that session's inbox.
- The diff pane reviews one hunk at a time: Enter toggles `accepted`,
  `r` reverts the hunk through `git apply --reverse`, `n` adds a note
  that rides to the model with the next prompt, and marks persist in
  `.summonr/review.json`.

- A workspace `SYSTEM.md` loads only when the workspace is trusted
  (`--no-trust` skips it). `~/.summonr/SYSTEM.md` still applies
  everywhere.
- `~/.summonr` is created mode `0700` and existing directories are
  tightened to it.

### Fixed

- A hook that writes a lot to stderr, or more than 64 KB to either
  stream, no longer stalls the tool call until the job limit.
- A stream that produces no text, reasoning, or tool call (only
  keep-alives or empty delta frames) now fails after the endpoint's
  idle timeout and is retried. Before, a stalled GLM stream held a turn
  until the work-time limit (858 s in one benchmark trial).
- Leaving plan mode removes the sandbox plan mode turned on. Before, a
  session that had visited plan mode kept the OS sandbox in auto mode, and
  writes to `/tmp`, `~/.cargo`, and `~/.summonr` failed with
  `Operation not permitted`.
- A non-empty `policy.toml` with an uncontained session warns at startup
  that its rules are not enforced.
- `Endpoint`'s debug form reports `api_key`, `oauth`, `env`, and
  `headers` as presence flags and key names, never values.

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
| `/approve` | `/permissions approve` |
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
