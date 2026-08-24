# Summoner

`summon` is a local coding harness. You point it at a model. OpenAI
`POST /v1/chat/completions` hosts work (glm, kimi, grok, llama.cpp,
config). Anthropic Messages and Gemini use their native APIs. Auth is
an API key or an OAuth2 token that becomes a bearer. The command is `summon grok` or `summoner grok`.

What it actually does: it holds the job so the model does not have to.
That job is Work. Plan, execute, verify. You keep the objective. The
model edits this directory (hash-anchored, cannot leave the workspace).
A check command you named is the pass. Model prose is not.

0.1.0 alpha. Private git. Not on crates.io.

## Requirements

Rust 1.90. macOS or Linux. SSH to `ThomGardiner/sigil` (TUI crates).
Windows is unsupported.

## Install

```sh
cargo install --path . --force
summon -V
summoner -V
```

Expect `summon 0.1.0` (same binary as `summoner`). Build fails without Sigil.

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

Then `summon local`. `summon endpoints` probes `GET /models` for each
name so a dead host fails before the first tool round. `summon login NAME`
runs RFC 8628 device-code login when that row has `device_url`.

Built-ins: chat/completions seats `glm`, `glm-intl`, `qwen`, `kimi`,
`grok`, `copilot`, `openai`, `openrouter`; native wires `anthropic`
(Messages) and `gemini`. `summon login copilot` is GitHub device login;
chat then exchanges that token for a Copilot session token. Kimi is a
key (`KIMI_*` or `~/.kimi-code/credentials`). Keys are the host's own
env (`XAI_API_KEY`, `ZAI_*`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`,
`ANTHROPIC_API_KEY`, `GEMINI_API_KEY`). Overlay `wire = "anthropic"`
on any `[endpoint.NAME]` row. See [`config.example.toml`](config.example.toml).

```sh
summon --goal "make cargo test pass"
summon --steps PLAN.md
```

`--goal` keeps one objective until `.mage/check.toml` is green and a
judge says stop. `--steps` hands the model one numbered item at a
time. Both isolate in a git worktree (`<repo>-worktrees/<id>`) when
you are in a repo. Exit 0 fast-forwards your branch and removes the
tree. `--here` stays put. The `agent` tool runs one nested agent
(depth 1) and returns its final text. `diagnostics` and `definition`
talk to one language server (`definition` also lists references and
applies rename). `grep` takes `glob`; an empty pattern lists matching
files. Defaults pick the first popular binary on PATH for the tree
(Biome before tsserver on a Node repo, Ruff before pyright on Python).
Set `[lsp] command` in `.mage/config.toml` to plug in anything else.

Unknown flags exit 1. A red verify gate exits 2. A budget abort
exits 3.

## Limits

- File tools cannot leave the workspace. `run_command` is a string
  policy unless you pass `--sandbox` (Seatbelt or bubblewrap). Missing
  sandbox backend is a hard stop, not an open run.
- Home is still `~/.mage`. Env is still `MAGE_*`.
- Not crates.io. No Sigil SSH, no build.
- An older `mage` binary may still be on `PATH`. Use `summon`.

## Docs

- [Work](docs/architecture.md)
- [`config.example.toml`](config.example.toml)
- [`policy.example.toml`](policy.example.toml)
