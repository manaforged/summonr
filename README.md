# Summoner

Summoner is a local coding harness. You point it at a model, it edits
the current directory with hash-anchored tools, and a check command you
named decides whether a turn passed. Model prose never does.

![Summoner splash screen with a configured DeepSeek endpoint selected](docs/assets/summoner-splash.png)

`summon grok` starts a session on one endpoint. With no name it uses
the `default` row in `~/.mage/config.toml`. `summoner` is the same
binary. Hosts that speak OpenAI `POST /v1/chat/completions` work as
configured rows; Anthropic Messages and Gemini use their own wires.

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
summon -V
```

Prints `summon 0.1.0 (<git revision>)`. `summoner` is the same binary
under a second name. Not on crates.io.

## Usage

```sh
summon grok
summon kimi -p "What is in Cargo.toml?"
summon --goal "make cargo test pass"
summon --steps PLAN.md
```

Built-in endpoints: `zai`, `zai-cn`, `glm`, `glm-intl`, `qwen`, `kimi`,
`grok`, `copilot`, `openai`, `openrouter`, `anthropic`, and `gemini`.
Each reads its host's environment variable (`XAI_API_KEY`,
`ANTHROPIC_API_KEY`, and so on) or a key you store with
`summon login NAME`. Keys live in `~/.mage/credentials.toml`, a
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
Both modes run in a git worktree under `<repo>-worktrees/<id>` and
fast-forward your branch on exit 0. `--here` stays in place.

`--plan` gives the model a read-only workspace. `--gui` opens the same
session in a desktop window when the binary was built with
`--features gui`.

## Limits

- Shell commands run in Seatbelt, bubblewrap, or the Windows
  AppContainer with the network denied. A missing backend is a hard
  stop, not a fallback. `--net` allows sockets; `--no-sandbox` removes
  containment and file policy together.
- A turn with no configured check completes with `verified: false`.
  A red check exits 2. A budget breach, a stalled goal, or an aborted
  turn exits 3.
- Language servers (Rust, TypeScript, Python, Go) run outside the
  sandbox with your account's permissions. Turn the `lsp` module off
  when you do not trust the project.
- No MCP client for third-party servers, no hook API, no plugin loader.
  Modules are compiled in.
- Config lives under `~/.mage` and `MAGE_*` variables; the rename to
  the Summoner name is not done.

## Docs

- [Reference](docs/reference.md): every flag, key, slash command,
  pane, policy rule, workflow seat, child session, and the agent room.
- [Work](docs/architecture.md): how a turn plans, executes, and verifies.
- [`config.example.toml`](config.example.toml) and
  [`policy.example.toml`](policy.example.toml).

MIT. Copyright (c) 2026 Thomas Gardiner.
