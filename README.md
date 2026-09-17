# summonr

A terminal coding agent written in Rust. It runs in 18 MB, sends a 513-character system prompt, and stops when your test command passes.

![summonr in a terminal](docs/assets/summoner-splash.png)

The terminal UI is the supported surface. The `--gui` desktop window is an experiment outside that scope.

## Requirements

macOS or Linux, Git, and Rust 1.98. Windows builds run, but the sandbox there is unfinished.

## Install

```sh
git clone https://github.com/manaforged/summonr
cd summonr
cargo install --path . --locked
```

## Usage

Set a key and start a session:

```sh
export ANTHROPIC_API_KEY=...
summonr anthropic
```

Run one prompt and exit:

```sh
summonr anthropic -p "why does cargo test fail?"
```

Work until the tests pass:

```sh
mkdir -p .mage
echo 'check = "cargo test"' > .mage/check.toml
summonr anthropic --goal "make cargo test pass"
```

`--goal` keeps going until the check exits 0 and a second model call agrees the goal is met. Exit codes: 0 done, 1 harness error, 2 check still red, 3 stopped short on a budget, a stall, or an abort. CI can branch on them.

Built-in endpoints include `anthropic`, `openai`, `gemini`, `grok`, `deepseek`, `openrouter`, `kimi`, and `copilot`. `summonr endpoints` lists them all with their key status. Any OpenAI-compatible server is one row in `~/.mage/config.toml`:

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8081/v1"
model = "qwen3"
api_key = "local"
```

## What it costs to run

Same model, same three tasks, five samples each, every harness on its defaults, tokens counted at a logging proxy. GLM-5.3-Flash, 2026-09-05. Medians over solved runs.

| Harness | Solved | Harness RSS | System prompt | Prompt tokens / task |
| --- | ---: | ---: | ---: | ---: |
| summonr | 13/13 | 18 MB | 513 chars | 75,149 |
| pi | 12/12 | 138 MB | 16,657 | 77,921 |
| goose | 15/15 | 110 MB | 1,444 | 101,643 |
| hermes | 14/14 | 160 MB | 942 | 136,433 |
| opencode | 13/13 | 738 MB | 2,096 | 237,564 |
| omp | 13/14 | 714 MB | 18,365 | 364,298 |

Three tasks is a smoke test, not a benchmark. A Terminal-Bench 2.0 run against the same harnesses is in progress. The method, proxy, and table script live in `bench/`, and the table will publish whatever it shows.

Where the savings come from: the system prompt is one paragraph. The model sees four core tools every round (`read`, `edit`, `write`, `run_command`) plus only the modules you turned on. The largest context on a solved task was 29k characters; opencode carried 85k for the same work.

## What it has that pi doesn't

- `--goal`: your check command decides done, then a second model call confirms it.
- MCP servers from your Claude Code and Codex configs load as is. A repo's own `.mcp.json` servers wait for your approval.
- Every turn is a git checkpoint. `/undo` drops the last turn and restores the files.
- Six identical results from the same call end the turn. In the terminal UI it asks first.
- `summonr acp` lets editors that speak the Agent Client Protocol drive a session.
- Force pushes, `sudo`, and `rm -rf /` are refused. Other risky commands ask first. With `-p`, nothing risky runs.

## Limits

- No plugin API. Extending it means changing the Rust source. If you want to write extensions, pi is the better tool.
- It doesn't remember anything between sessions.
- The Windows sandbox is unfinished.
- Config lives under `~/.mage`, left over from the project's old name.
- On a headless Linux box the keychain may not answer at startup. Set `secrets = "file"` in `~/.mage/config.toml` or pass keys through the environment.

## Docs

- [Reference](docs/reference.md): flags, slash commands, config keys, exit codes
- [Architecture](docs/architecture.md): how a turn runs
- [Changelog](CHANGELOG.md)
- Example configs: [`config.example.toml`](config.example.toml), [`policy.example.toml`](policy.example.toml)

MIT licensed. A [Manaforge Technologies](https://manaforge.tech/) project.
