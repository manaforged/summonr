# Summonr

Summonr is a local coding harness. You point it at a model, it edits
the current directory with hash-anchored tools, and a check command you
named decides whether a turn passed. Model prose never does.

![Summonr splash screen with a configured DeepSeek endpoint selected](docs/assets/summoner-splash.png)

`summonr grok` starts a session on one endpoint. With no name it uses
the `default` row in `~/.mage/config.toml`. Hosts that speak OpenAI
`POST /v1/chat/completions` work as configured rows; Anthropic Messages
and Gemini use their own wires.

## Requirements

- macOS or Linux, with Git and Bash on `PATH`. Linux also needs
  bubblewrap and permission to create user namespaces.
- The Rust toolchain pinned in `rust-toolchain.toml` (1.98.1) for a
  source build. Cargo fetches the TUI crates from the public Sigil
  repository.
- Windows is experimental: PowerShell inside an AppContainer. Git for
  Windows and user-installed toolchains do not run in that sandbox yet.

## Install

```sh
cargo install --path . --locked
summonr -V
```

Prints `summonr 0.1.0 (<git revision>)`. Not on crates.io.

To run this checkout instead of a released copy, link your `PATH` at the dev
wrapper once:

```sh
ln -sfn "$PWD/scripts/summon-dev" ~/.cargo/bin/summonr
ln -sfn "$PWD/scripts/summon-dev" ~/.cargo/bin/summon
ln -sfn "$PWD/scripts/summon-dev" ~/.cargo/bin/summoner
summonr -V
```

`scripts/summon-dev` rebuilds `--release` when `src/` or the Cargo files are
newer than `target/release/summonr`, then execs that binary. `summon` and
`summoner` remain aliases of the same wrapper. `sh scripts/dev-link.sh`
links straight at the binary instead if you would rather rebuild by hand.

## Usage

```sh
summonr grok
summonr kimi -p "What is in Cargo.toml?"
summonr --goal "make cargo test pass"
summonr --steps PLAN.md
```

Built-in endpoints: `zai`, `zai-cn`, `glm`, `glm-intl`, `qwen`, `kimi`,
`grok`, `copilot`, `openai`, `openrouter`, `muse`, `anthropic`, `gemini`,
`zen`, `go`, and `opencode`.
Each reads its host's environment variable (`XAI_API_KEY`,
`ANTHROPIC_API_KEY`, `OPENCODE_API_KEY`, and so on) or a key you store with
`summonr login NAME`. Keys live in `~/.mage/credentials.toml`, a
ChaCha20-Poly1305 blob whose wrapping key sits in the OS keychain.
`cat` on that file shows nothing useful. Add any other host in
`~/.mage/config.toml`:

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8081/v1"
model = "whatever-the-server-calls-it"
api_key = "local"
```

`--goal` keeps one objective until every command in `.mage/check.toml`
exits 0 and a judge model answers `done`. `--steps` hands the model one
numbered item from `PLAN.md` at a time and gates each on its `Run` line.
Both modes edit this directory. `--worktree` isolates a session under
`<repo>-worktrees/<id>` and fast-forwards your branch on exit 0.

`--plan` gives the model a read-only workspace. `--gui` opens the same
session in a desktop window when the binary was built with
`--features gui`.

## Tools

The model sees these every turn (from `src/tools/catalog.rs`). Reads
return `N:hash|text` lines; edits that miss a hash fail before write.

| Tool | Job |
| --- | --- |
| `read` | File (tagged lines), directory, image, or search (`pattern` / `glob`) |
| `edit` | Change an existing file (replace, insert, delete; `edits` for several at once) |
| `write` | Create or overwrite a file |
| `run_command` | Sandboxed shell; `background` for jobs |

On when present: `skill` (a `.mage/skills` catalog), `lsp` (`lsp`
module), `web` (`search` module), `task` (`task` module), `bus` (room).
Plan mode keeps `read` and `run_command`, plus `skill` and `web`. There
is no MCP client.

## Limits

- Shell commands run in Seatbelt, bubblewrap, or the Windows
  AppContainer with the network denied. A missing backend is a hard
  stop, not a fallback. `--net` allows sockets; `--no-sandbox` removes
  containment and file policy together. Writes are allowed under the
  launch directory, `[sandbox] write`, and `--add-dir`. Auto mode grants
  an extra-root write for this session and retries. `/approve on` asks
  first. `[ always ]` and `/write PATH` append the path to
  `[sandbox] write`.
- A turn with no configured check completes with `verified: false`.
  A red check exits 2. A budget breach, a stalled goal, or an aborted
  turn exits 3.
- Language servers (Rust, TypeScript, Python, Go) run outside the
  sandbox with your account's permissions. Turn the `lsp` module off
  when you do not trust the project.
- No MCP client for third-party servers, no hook API, no plugin loader.
  Modules are compiled in.
- Config lives under `~/.mage` and `MAGE_*` variables. The command and
  crate are `summonr`; the config directory is unchanged.

## Docs

- [Reference](docs/reference.md): every flag, key, slash command,
  pane, policy rule, workflow seat, child session, and the agent room.
- [Work](docs/architecture.md): how a turn plans, executes, and verifies.
- [`config.example.toml`](config.example.toml),
  [`policy.example.toml`](policy.example.toml), and
  [`theme.example.toml`](theme.example.toml).

MIT. Copyright (c) 2026 Thomas Gardiner.
