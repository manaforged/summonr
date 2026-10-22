# summonr

A terminal coding agent for any model that keeps working until your check command passes.

- **Light on tokens and memory**: median input tokens per solved task under half, and median memory under a third, of every agent paired with it on the same model ([benchmarks](#benchmarks)).
- **Any model, your plan or your key**: 28 providers built in, sign-in for ChatGPT and SuperGrok plans, and any OpenAI-compatible server.
- **Works until it passes**: `--goal` runs until your check command exits 0 and a second model call agrees the goal is met, inside a time or token budget you set.
- **Sessions that talk**: message any running session with `@name`, even one on another model or provider. [Try it](#coordinate-work-live).
- **Parallel children that land their own edits**: each implement child works in its own checkout, and the parent applies each finished one.
- **Browses like you do**: drives your installed Chrome, Edge, or Brave in a profile of its own that stays signed in, and searches the web with no API key. Turn on `browser` and `search` under `[modules]`.
- **Works while you're away**: schedule a goal for 3 AM with `summonr cron`, fire one from a webhook, or text it from Telegram. It tells you whether the check passed.
- **Remembers**: tell it a preference once and every later session in that repository knows it. The notes are plain Markdown you can edit.

[Quick start](docs/guide/quickstart.md) | [Docs](docs/guide/README.md) | [Reference](docs/guide/reference.md) | [Scripting API](docs/api.md) | [Benchmarks](#benchmarks)

![summonr fixing two bugs in calc.py and running pytest, captured from a session](docs/guide/assets/summonr-session.png)

## Install

macOS and Linux:

```sh
curl -fsSL https://summonr.dev/install.sh | sh
```

Windows:

```powershell
powershell -c "irm https://summonr.dev/install.ps1 | iex"
```

Each [release](https://github.com/manaforged/summonr/releases) ships binaries for macOS (Apple silicon), Linux (x86_64 and arm64, glibc or static musl), and Windows (x86_64). The installer downloads the archive's `.sha256` file from the same release, checks the archive's SHA-256 against it, and installs `summonr` to `~/.local/bin`, or `%LOCALAPPDATA%\Programs\summonr\bin` on Windows. Set `SUMMONR_VERSION` to pick a release tag, `SUMMONR_INSTALL_DIR` to change the directory, and `SUMMONR_LIBC=gnu` for the glibc build on Linux.

To install by hand, download the archive and `SHA256SUMS` from [GitHub Releases](https://github.com/manaforged/summonr/releases), then check the archive:

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing
```

On Windows, compare the output of `Get-FileHash <file> -Algorithm SHA256` with the line in `SHA256SUMS`. A checksum from the same release catches a corrupted download, not a tampered release. To check where a file came from, verify its build attestation:

```sh
gh attestation verify <file> --repo manaforged/summonr
```

To update, run `summonr update`. To uninstall, delete the binary: `~/.local/bin/summonr` or `%LOCALAPPDATA%\Programs\summonr\bin\summonr.exe`.

### Build from source

You need Rust 1.98 or later, Git, CMake, and a C and C++ compiler. On Windows, use the MSVC Rust toolchain and Visual Studio C++ Build Tools.

```sh
cargo install --git https://github.com/manaforged/summonr --locked
```

Run the same command to update. To uninstall, run `cargo uninstall summonr`.

## First run

Run `summonr` in a project directory, pick a provider, and paste a key or sign in to a coding plan.

The [quick start](docs/guide/quickstart.md) walks through your first session and your first goal run.

Commands run as your account with no sandbox; see [Safety and policy](docs/guide/safety.md).

## Desktop window

`summonr --gui` opens the same session in a Sigil desktop window. The release binaries and the default build leave it out; build with the `gui` feature:

```sh
cargo install --git https://github.com/manaforged/summonr --locked --features gui
summonr --gui
```

## Usage

```sh
summonr anthropic                                   # start a session
summonr anthropic -p "why does cargo test fail?"    # one prompt, then exit
summonr anthropic --goal "make cargo test pass" --max-secs 1800
```

- [Models and sign-in](docs/guide/models.md): the built-in providers, coding-plan sign-in, and your own OpenAI-compatible server with `summonr login NAME --url URL`.
- [Goals and checks](docs/guide/goals.md): `--goal`, budgets, exit codes, numbered steps, `--worktree`, and one-shot `-p` runs.
- [Using the terminal](docs/guide/terminal.md): `/undo`, child sessions, `summonr export`, and memory.
- [Extending summonr](docs/guide/extending.md): skills, MCP servers, hooks, CLIs, `summonr install`, `summonr acp`, and project instructions.

## Coordinate work live

Your sessions can message each other with `@name`, even when they run different models. Start two in separate terminals in your project:

```sh
summonr anthropic --name builder
summonr openai --name tests
```

In the builder, type:

```text
@tests I'm updating the parser. Can you cover the edge cases?
```

It lands in the tests session's inbox, and the tests session replies with `@builder`. Type `@` to pick a live session, or open `/pane peers` to see who is running. Claude Code, Codex, or a script can join through `summonr mail`; see [Mail](docs/guide/reference.md#mail).

## Benchmarks

Terminal-Bench 2.1, at high effort, with every agent in a pair on the same model. In every pair, summonr used under half the input tokens per solved task and under a third of the memory; resolution rates were within 7 points of each other. The plan-pair tables use the earliest 3 trials per arm and task from larger side-by-side jobs covering all 89 tasks. Those historical jobs include summonr binary switches, and their judge pass is unfinished.

GPT-6.1 Sol on a ChatGPT plan:

| Agent | Input tokens per solved task | Memory | Cost per trial | Resolution rate |
| --- | ---: | ---: | ---: | ---: |
| summonr 0.1.0 | 58,976 | 27 MB | $0.078 | 85.8% |
| Codex 0.162.1 | 182,691 | 276 MB | $0.105 | 79.0% |

Grok 4.7 on a SuperGrok plan:

| Agent | Input tokens per solved task | Memory | Resolution rate |
| --- | ---: | ---: | ---: |
| summonr 0.1.0 | 373,831 | 27 MB | 74.2% |
| Grok Build 1.0.52 | 803,466 | 146 MB | 70.4% |

GLM 5.3 Flash through z.ai, one try on each of 86 tasks; OpenCode ran in a later job than summonr and pi, and its memory includes the server it starts (86 MB without it):

| Agent | Input tokens per solved task | Memory | Resolution rate |
| --- | ---: | ---: | ---: |
| summonr 0.1.0 | 95,271 | 24 MB | 73.3% |
| pi 1.0.0 | 240,762 | 134 MB | 72.1% |
| OpenCode 2.0.22 | 228,578 | 394 MB | 67.4% |

Medians, as each agent reported them: input over the tasks that agent solved, cost per trial at API list price. Method and caveats: [`BENCHMARKS.md`](BENCHMARKS.md).

Across all 5 ChatGPT trials per task, summonr scored 374/445 and Codex 359/445 after source-lookup voids. The paired solve-difference interval includes zero: 0.03 (-0.01–0.08). The paired input-per-solved-trial ratio remains 0.33 (0.28–0.38); execution time is 1.17× Codex's. These results support an efficiency claim, not general task-solving superiority. [Per-trial data and reproduction](bench/results/README.md).

The first-round prefix is one short system paragraph, the JSON schemas of the advertised tools, an optional skill catalog, and `AGENTS.md`. Six core tools are advertised every round (`read`, `search`, `edit`, `write`, `run_command`, `todo`), plus `ask` in an interactive session and the modules you turned on. MCP schemas join the prefix only for servers you configure.

## Limits

Commands run as your account with no sandbox, and only irreversible destruction asks first. [Safety and policy](docs/guide/safety.md) lists what asks, `confirm = "risky"`, and `--strict`.

MCP limits, project instructions, and why there is no in-process plugin loader are in [Extending summonr](docs/guide/extending.md). Memory and `summonr export` are in [Using the terminal](docs/guide/terminal.md), and key storage is in [Models and sign-in](docs/guide/models.md). Config lives under `~/.summonr`.

## Docs

| Read | For |
| --- | --- |
| [Quick start](docs/guide/quickstart.md) | Install, sign in, and run a first goal. |
| [User guide](docs/guide/README.md) | Task pages in reading order: models, goals, schedules, the terminal, extending, and safety. Build the book with `mdbook serve docs`. |
| [Reference](docs/guide/reference.md) | Flags, config keys, exit codes, and environment variables. |
| [API map](docs/api.md) | For agents and tools: each job mapped to its symbol or command, the error model, and what is off-limits. |
| [Architecture](docs/guide/architecture.md) | How it works inside. |
| [Contributing](CONTRIBUTING.md) | Build, check, and test commands, and where each kind of change goes. |

The [changelog](CHANGELOG.md) lists the changes in each release. Example configs: [`config.example.toml`](config.example.toml) and [`policy.example.toml`](policy.example.toml).

## Security

Report vulnerabilities as described in [SECURITY.md](SECURITY.md).

## Contributing

Pull requests are by invitation; open an [issue](https://github.com/manaforged/summonr/issues) for bugs and requests.

## License

MIT. See [LICENSE](LICENSE).

summonr is built by Manaforge Technologies LLC.
