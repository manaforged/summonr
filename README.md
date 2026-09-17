# summonr

A terminal coding agent where your test command, not the model, decides when the work is done.

![summonr in a terminal](docs/assets/summoner-splash.png)

## Install

You need macOS or Linux, Git, and Rust 1.98.

```sh
git clone https://github.com/thomgardiner/summonr
cd summonr
cargo install --path . --locked
```

Windows support is experimental.

## Use it

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

`--goal` keeps going until the check exits 0 and a second model call agrees the goal is met. It exits 0 when done, 2 if the check is still red, and 3 if it stopped early.

There are 20 built-in endpoints, including `anthropic`, `openai`, `gemini`, `grok`, `openrouter`, and `kimi`. `summonr endpoints` lists them all. Any OpenAI-compatible server works as a row in `~/.mage/config.toml`:

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8081/v1"
model = "qwen3"
api_key = "local"
```

## What's different

- Edits carry a hash of the lines the model read. If the file changed since, the edit fails instead of landing in the wrong place.
- Commands run with your permissions, but `rm -rf`, `git reset --hard`, and force pushes ask first. With `-p` they don't run at all.
- Every turn saves a git checkpoint. `/undo` drops the last turn and puts the files back.
- If the model gets the same result from the same call six times, summonr stops the turn. In the terminal UI it asks you first.
- MCP servers from your Claude Code and Codex configs just work. A repo's `.mcp.json` servers load only after you approve them.
- `summonr acp` lets editors that speak the Agent Client Protocol drive a session.

## Why you might not want it

- There's no plugin API. Extending it means changing the Rust source.
- It doesn't remember anything between sessions.
- The Windows sandbox is unfinished.
- Config lives under `~/.mage`, left over from the project's old name.

## Docs

- [Reference](docs/reference.md): flags, slash commands, config keys, exit codes
- [Architecture](docs/architecture.md): how a turn runs
- Example configs: [`config.example.toml`](config.example.toml), [`policy.example.toml`](policy.example.toml)

MIT licensed.
