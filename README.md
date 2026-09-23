# summonr

A terminal coding agent written in Rust that works until your test command passes, and loads no third-party code into its process.

![summonr in a terminal](docs/guide/assets/summonr-splash.png)

Using a coding agent with summonr? Give it [docs/api.md](docs/api.md). `summonr --gui` is the same session in a Sigil desktop window (`cargo install --path . --locked --features gui`).

## Requirements

- macOS, Linux, or Windows.
- Git.
- Rust 1.98.
- For `--sandbox`: Seatbelt on macOS, or bubblewrap on Linux. Windows AppContainer is unfinished and needs a `--features windows-sandbox` build. Git for Windows and user-installed toolchains stay blocked, so don't treat `--sandbox` as containment on Windows.

## Install

```sh
git clone https://github.com/manaforged/summonr
cd summonr
cargo install --path . --locked
```

`summonr --gui` opens the same session in a Sigil desktop window. It needs a build with the `gui` feature:

```sh
cargo install --path . --locked --features gui
summonr --gui
```

## Usage

Set a key and start a session:

```sh
export ANTHROPIC_API_KEY=...
summonr anthropic
```

If you have a coding plan, sign in instead of using a metered key:

```sh
summonr login              # every plan and key, with its status
summonr login claude-pro   # Claude Pro or Max, through the browser
summonr claude-pro
```

Run one prompt and exit:

```sh
summonr anthropic -p "why does cargo test fail?"
```

Work until the tests pass:

```sh
mkdir -p .summonr
echo 'check = "cargo test"' > .summonr/check.toml
summonr anthropic --goal "make cargo test pass"
```

`--goal` continues until the check exits 0 and a second model call agrees that the goal is met. Exit codes: 0 done, 1 harness error, 2 check still red, 3 stopped short on a budget, a stall, or an abort. CI can branch on them.

Next commands:

- `/undo` drops the last turn and restores the files. Every turn is a git checkpoint.
- `/modules task on` turns on child sessions: batch, background, poll, and kill.
- `/modules mcp on`, then a `[mcp.NAME]` table in `~/.summonr/config.toml`, connects an MCP server.
- `summonr acp` lets an editor that speaks the Agent Client Protocol drive a session.
- `summonr export` writes a session as Markdown.
- `summonr endpoints` lists every endpoint with its key status.

Built-in endpoints include `anthropic`, `openai`, `gemini`, `grok`, `deepseek`, `ollama`, `openrouter`, `kimi`, `copilot`, `together`, `fireworks`, `deepinfra`, and `minimax`. Any OpenAI-compatible server is one table in `~/.summonr/config.toml`:

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8081/v1"
model = "qwen3"
api_key = "local"
```

Optional capabilities are compiled modules; `summonr modules` lists them. You extend summonr outside the process with a skill file, an MCP server, a hook, or a CLI on `PATH`. `summonr add PATH` installs a directory of those files. See [Extending summonr](docs/guide/extending.md).

## Benchmarks

Same model, same three tasks, five samples each, every harness on its defaults, tokens counted at a logging proxy. GLM-5.3-Flash, 2026-09-05. Medians over solved runs.

| Harness | Solved | Agent RSS* | Prompt tokens / task |
| --- | ---: | ---: | ---: |
| summonr | 13/13 | 18 MB | 75,149 |
| pi | 12/12 | 138 MB | 77,921 |
| goose | 15/15 | 110 MB | 101,643 |
| hermes | 14/14 | 160 MB | 136,433 |
| opencode | 13/13 | 738 MB | 237,564 |
| omp | 13/14 | 714 MB | 364,298 |

\* RSS of the agent's main process in `-p` print mode. Language servers, MCP
servers, and subagents run as separate processes and are not counted. Median
session length here is about 10 s, so read these as startup and short-task
numbers.

Summonr's fixed prefix is larger than pi's. In the graded 20-task suite
(`BENCHMARKS.md`), the first request carried a 7,081 to 7,089 B system message
plus 5,267 B of tool schemas for summonr (7 tools), and 4,511 to 4,527 B plus
3,024 B for pi (4 tools). Part of summonr's system message in that run was the
operator's global rules files, which `grade.sh` now keeps out.

Three tasks is a smoke test, not a benchmark. A Terminal-Bench 2.0 run against the same harnesses is in progress. The method, the proxy, and the table script are in [`bench/`](bench/README.md), and the table will publish whatever it shows. The graded 16-task suite with deterministic verifiers — its full scorecard and limits — lives in [`BENCHMARKS.md`](BENCHMARKS.md).

Where the savings come from: the first-round prefix is one short system paragraph, the JSON schemas of the advertised tools, an optional skill catalog, and `AGENTS.md`. Five core tools are advertised every round (`read`, `edit`, `write`, `run_command`, `todo`), plus the modules you turned on. `mcp` and `task` are off by default, so unused MCP schemas never sit in the prefix. The largest context on a solved task was 29k characters; opencode carried 85k for the same work. The 18 MB figure is `-p` print mode; the terminal UI has not been measured.

## Limits

- summonr refuses force pushes, `sudo`, and `rm -rf /`. Other risky commands ask first. With `-p`, nothing risky runs.
- Six identical results from the same call end the turn. The terminal UI asks first.
- The `mcp` and `task` modules are off by default. Claude Code and Codex MCP configs load only when `import_mcp = true` is set in `~/.summonr/config.toml`. A project `.mcp.json` waits for approval.
- A project can turn a module off. A project can't turn a module on.
- `AGENTS.md` in a trusted workspace is the project instruction file. summonr doesn't read `CLAUDE.md`.
- `summonr export` writes Markdown. summonr doesn't import transcripts from other tools.
- There is no in-process package loader, `dlopen`, or guest scripting. Write a skill, an MCP server, a hook, or a CLI.
- Repository search is `read` with `pattern` or `glob`, or `rg` through `run_command`. There is no built-in code graph.
- There is no automatic memory between sessions unless you add the memory skill.
- Config is under `~/.summonr`.
- On a headless Linux host, the keychain may not answer at startup. Set `secrets = "file"` in `~/.summonr/config.toml`, or pass keys through the environment.

## Docs

| Read | For |
| --- | --- |
| [User guide](docs/guide/README.md) | Task pages in reading order. Build the book with `mdbook serve docs`. |
| [Reference](docs/guide/reference.md) | Every flag, config key, exit code, and environment variable. |
| [API map](docs/api.md) | For agents and tools: each job mapped to its symbol or command, the error model, and what is off-limits. |
| [Architecture](docs/guide/architecture.md) | How it works inside. |

The [changelog](CHANGELOG.md) lists the changes in each release. Example configs: [`config.example.toml`](config.example.toml) and [`policy.example.toml`](policy.example.toml).

## License

MIT. See [LICENSE](LICENSE). A [Manaforge Technologies](https://manaforge.tech/) project.
