# Changelog

## Unreleased

- Overlay chrome (options, search, sessions, composer, meters) uses
  Sigil `list_row`, `button`, `card`, `hint_bar`, and `notice_band`.
- The advertised tool catalog freezes after the first `prefix_hash` in
  the process. `/modules` writes apply on the next process.
- The TUI skips a full paint when the session is idle.
- `/plan accept` enters work and runs the plan in that turn. Work is not
  a sibling of plan: without a selected plan, a work request stays in
  plan mode. `shift+tab` switches auto and plan only.
- The `task` module is off by default. Turn it on with `/modules task on`
  when the work has disjoint paths. The harness prompt tells the model
  to spawn a child only then, and that the completion check decides done.
- Transcript pictures keep the full preview bitmap in the overlay.
  A picture is 48 rows tall at most.
- A fenced `chart` block, or mermaid `xychart` / `xychart-beta`,
  renders as labeled bars.
- Crate and command are `summonr`. `summon` and `summoner` remain PATH
  aliases of the same binary. Config stays under `~/.summonr`.
- The model sees four core tools every round: `read` (file, directory,
  image, or search), `edit`, `write`, and `run_command`. Optional
  modules add `skill`, `lsp`, `web`, `task`, and `bus`. Old names still
  run and are not advertised.
- Ordinary chat executes immediately. `shift+tab` and `/plan` enter
  plan mode. The reply is saved to `.summonr/plan.md`. Sandboxed network
  is on by default; `--no-net` denies it. Destructive writes still ask.
- The four core tools got deeper without growing: `read` search takes
  `literal`, `caseSensitive`, and `include`; `edit` takes a `files`
  batch and a `to` rename; `write` takes a `files` batch;
  `run_command` takes `workdir` and names the full log for a long
  output.
- `web` fetches as markdown, text, or html, and keeps the full page
  when the output is truncated.
- Plan mode is read-only. `edit` and `write` are unavailable. The model
  inspects with `read` and `run_command`, then proposes diffs in the
  answer.
- `--goal`, `--steps`, and `implement` children edit the directory you
  launched from. Pass `--worktree` to isolate the whole session.
- A sandboxed write or network denial pauses in the interface:
  `[ allow ]`, `[ always ]`, or `[ deny ]`. `[ always ]` appends the
  path to `[sandbox] write`. `/write PATH` and `/net on` / `/net once`
  do the same without a prompt.
- Built-in endpoints add `together`, `groq`, `cerebras`, `mistral`, and
  `muse` (chat/completions). They set no default model: name one with
  `--model`. `muse` defaults to `muse-spark-1.3`.
- Built-in OpenCode seats: `zen`, `go`, and `opencode`
  (`chat/completions`). Keys are `OPENCODE_API_KEY` and
  `OPENCODE_GO_API_KEY`. Every wire sends `x-opencode-session`, and
  `responses` is a selectable wire.
- The Sigil dependency fetches over https, so a checkout needs no SSH
  key once the repository is visible.
- Streamed reasoning is italic `thought` text on the canvas. There is no
  separate reasoning band.
- The interface ships the `space` preset only. `/theme` reloads theme files.

## 0.1.0 - 2026-09-10

First public release of the Mage harness under the Summoner name.

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
- A `Retry-After` header on a 429 or 5xx reply sets the retry wait, up to
  600 s, instead of the doubling backoff.
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
  and Job Objects. Cancellation stops descendants and removes temporary
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
  turn cannot complete while one still runs. `--max-turns` also bounds a
  goal whose checks keep failing (exit 2). Every goal turn restates the
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
