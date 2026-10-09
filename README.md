# summonr

An open-source coding agent for any model, in your terminal or a desktop window. It does the same work as heavier harnesses on fewer tokens and a fraction of the memory: in our [Terminal-Bench 2.1 run](https://summonr.dev/benchmarks) it used 24 MB, where four other harnesses peaked between 146 and 505 MB. It loads no third-party code into its process.

![summonr fixing two bugs in calc.py and running pytest, captured from a session](docs/guide/assets/summonr-session.png)

## Install

macOS and Linux:

```sh
curl -fsSL https://summonr.dev/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://summonr.dev/install.ps1 | iex
```

Release binaries cover macOS (Apple silicon), Linux (x86_64, arm64, static musl), and Windows (x86_64). The installer downloads the archive's `.sha256` file from the same release, checks the archive's SHA-256 against it, and installs `summonr` to `~/.local/bin`, or `%LOCALAPPDATA%\Programs\summonr\bin` on Windows. Set `SUMMONR_VERSION` to pick a release tag and `SUMMONR_INSTALL_DIR` to change the directory.

To install by hand, download the archive and `SHA256SUMS` from [GitHub Releases](https://github.com/manaforged/summonr/releases), then check the archive:

```sh
shasum -a 256 -c SHA256SUMS --ignore-missing
```

On Windows, compare the output of `Get-FileHash <file> -Algorithm SHA256` with the line in `SHA256SUMS`. A checksum from the same release does not prove where the file came from. To check provenance, verify the build attestation that each release file has:

```sh
gh attestation verify <file> --repo manaforged/summonr
```

To update, run the installer again. To uninstall, delete the binary: `~/.local/bin/summonr` or `%LOCALAPPDATA%\Programs\summonr\bin\summonr.exe`.

From source (the same targets as the release binaries, Rust 1.98, Git, CMake, and a C and C++ compiler):

```sh
cargo install --git https://github.com/manaforged/summonr --locked
```

## First run

1. Run `summonr` in a project directory.
2. Type `/login` and pick a provider.
3. Paste the API key, or sign in to a coding plan in the browser.

To skip the prompt, set a key such as `ANTHROPIC_API_KEY` and run `summonr anthropic`.

Docs: [user guide](docs/guide/README.md), [reference](docs/guide/reference.md), and [docs/api.md](docs/api.md) for a coding agent that drives summonr.

## Sandbox

`--sandbox` uses Seatbelt on macOS and bubblewrap on Linux. Windows AppContainer is unfinished and needs a `--features windows-sandbox` build. Git for Windows and user-installed toolchains stay blocked, so don't treat `--sandbox` as containment on Windows.

## Desktop window

`summonr --gui` opens the same session in a Sigil desktop window. It needs a build with the `gui` feature:

```sh
cargo install --git https://github.com/manaforged/summonr --locked --features gui
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
summonr login openai       # sign in with ChatGPT Plus or Pro, or paste a key
summonr login grok         # sign in with SuperGrok or X Premium+, or paste a key
summonr openai
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

`--goal` continues until the check exits 0 and a second model call agrees that the goal is met. Exit codes: 0 done, 1 harness error, 2 check still red (summonr runs the check once more when a budget or an abort stops the run), 3 stopped short with no check configured, or with a passing check and an unmet goal. CI can branch on them.

Next commands:

- `/undo` drops the last turn and restores the files. Every turn is a git checkpoint.
- Child sessions are on by default in interactive sessions (`task` module): batch, background, poll, and kill. Children run on the same seat as the parent unless the workflow assigns the role elsewhere; `/modules task off` disables.
- An implement child works in `<repo>-deltas/<id>`, a checkout beside the repo. It shares `target/`, `node_modules`, the paths listed in `.summonr/linked` and `.agents/linked`, and the parent's git objects. It does not add a branch. Sharing those directories skips a second copy of the build cache. It does not limit how many checks run at once. The parent applies the child's edits. When two children change the same lines, that checkout stays on disk, shows as a conflict, and the other children still land. summonr never deletes a kept checkout. A research child stays in the directory you launched. `--worktree` still isolates the whole session on branch `summonr/<id>` under `<repo>-worktrees/<id>`.
- A `[mcp.NAME]` table in `~/.summonr/config.toml` connects an MCP server. The server starts on its first tool call and stops when idle.
- `summonr acp` lets an editor that speaks the Agent Client Protocol drive a session.
- `summonr export` writes a session as Markdown.
- `summonr endpoints` lists every endpoint with its key status.

Built-in endpoints include `anthropic`, `openai`, `gemini`, `grok`, `deepseek`, `qwen-plan`, `dashscope`, `kimi`, `minimax-plan`, `tencent-plan`, `volcengine-plan`, `nvidia`, `huggingface`, `cohere`, `perplexity`, `ollama`, `lmstudio`, `openrouter`, `copilot`, `together`, `fireworks`, `deepinfra`, and more: every coding plan and inference host is one row in the provider table (`src/endpoint/presets.toml`). Add any OpenAI-compatible server with one command:

```sh
summonr login local --url http://127.0.0.1:8080/v1   # picks the model when the server lists one
summonr login local --model qwen3-coder              # change its model
summonr login local --default                        # start on it when no endpoint is named
summonr local
summonr logout local --remove                        # remove it again
```

Pass `--model NAME` when the server lists several models. Or write the table in `~/.summonr/config.toml`:

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8080/v1"
model = "qwen3"
```

Optional capabilities are compiled modules; `summonr modules` lists them. You extend summonr outside the process with a skill file, an MCP server, a hook, or a CLI on `PATH`. `summonr install PATH` installs a plugin in the Claude Code layout: its skills, MCP servers, hooks, and `bin/`. See [Extending summonr](docs/guide/extending.md).

## Benchmarks

Terminal-Bench 2.1, 86 tasks, one pass, GLM 5.3 Flash at high effort through z.ai, every harness on its defaults. Runs `tb21-zai-1002d` (summonr, pi), `tb21-zai-1002f` (OpenCode, Hermes), and `tb21-zai-1003a` (OMP), 2026-10-02 to 2026-10-03; arms from different runs did not run side by side.

| Harness | Version | Tasks solved | Mean memory | Peak memory | Median input tokens, solved tasks | Median input tokens, all tasks | Median time, solved tasks |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| summonr | 0.1.0 | 63 | 24 MB | 24 MB | 95,271 | 194,193 | 219 s |
| pi | 1.0.0 | 62 | 134 MB | 146 MB | 240,762 | 212,337 | 368 s |
| OMP | 18.4.12 | 59 | 387 MB | 505 MB | 218,939 | 294,356 | 294 s |
| OpenCode | 2.0.22 | 58 | 394 MB | 480 MB | 228,578 | 253,726 | 341 s |
| Hermes | v2026.9.24 | 49 | 202 MB | 244 MB | 225,215 | 272,086 | 335 s |

Mean and peak memory are medians over trials. Peak is the median of each trial's highest sample. The figure counts the harness and every helper it started without a shell, where that run started a helper, and the harness process alone otherwise. summonr's 24 MB is the process. That run started no helper. The solved-tasks columns count only the tasks that harness solved. The all-tasks column counts all 86, solved and failed. summonr's failed tasks ran long, so its lead is smaller across all tasks than on solved ones: median input 194,193 against pi at 212,337, and a paired input ratio of 0.94 (0.76 to 1.17). On tasks summonr did not solve, the median was 551,156 input tokens and 29 requests, against 199,458 and 14 for pi. The lead in tasks solved over pi, OMP, and OpenCode is inside the 95% interval.

Method and limits: [`BENCHMARKS.md`](BENCHMARKS.md) and [summonr.dev/benchmarks](https://summonr.dev/benchmarks.html).

The first-round prefix is one short system paragraph, the JSON schemas of the advertised tools, an optional skill catalog, and `AGENTS.md`. Four core tools are advertised every round (`read`, `edit`, `write`, `run_command`), `todo` in steps and goal runs, plus the modules you turned on. MCP schemas join the prefix only for servers you configure.

## Limits

- Commands run without a prompt. Only irreversible destruction asks first: wiping the system, home, or project root, `mkfs`, `dd` to a device, `terraform destroy`, cluster deletes, and dropped database tables. `-p` never runs those. Set `confirm = "risky"` in `~/.summonr/policy.toml` to also ask before force pushes, `sudo`, pipes into a shell, and history rewrites; `--strict` refuses that whole list. Under `--sandbox` or `--strict`, your own `policy.toml` deny rules are hard refusals.
- Six identical results from the same call end the turn. The terminal UI asks first.
- The `mcp` module is on by default. A server starts on its first tool call, stops after two idle minutes, and runs inside `--sandbox` when that is on. A server with more than 8 KB of tool schemas is reached through one `mcp` tool, so large servers do not fill the prompt. `/server:prompt` runs a server prompt, `@server:uri` attaches a server resource, and a server can ask you to fill a form during a call. A reply over 4 MiB is dropped, and output over 50 KB is cut with the full text saved to a file. Claude Code and Codex MCP configs load only when `import_mcp = true` is set in `~/.summonr/config.toml`. A project `.mcp.json` server runs only after you set `enabled = true` for it.
- A project can turn a module off. A project can't turn a module on.
- A trusted workspace supplies `AGENTS.md`, `CLAUDE.md`, and `SYSTEM.md` as instructions; `--no-trust` ignores all three.
- `summonr export` writes Markdown. summonr doesn't import transcripts from other tools.
- There is no in-process package loader, `dlopen`, or guest scripting. Write a skill, an MCP server, a hook, or a CLI.
- Repository search is `read` with `pattern` or `glob`, or `rg` through `run_command`. There is no built-in code graph.
- There is no memory between sessions.
- Config is under `~/.summonr`.
- On a headless Linux host, the keychain may not answer at startup. Set `secrets = "file"` in `~/.summonr/config.toml`, or pass keys through the environment.

## Docs

| Read | For |
| --- | --- |
| [User guide](docs/guide/README.md) | Task pages in reading order. Build the book with `mdbook serve docs`. |
| [Reference](docs/guide/reference.md) | Flags, config keys, exit codes, and environment variables. |
| [API map](docs/api.md) | For agents and tools: each job mapped to its symbol or command, the error model, and what is off-limits. |
| [Architecture](docs/guide/architecture.md) | How it works inside. |
| [Contributing](CONTRIBUTING.md) | Build, check, and test commands, and where each kind of change goes. |

The [changelog](CHANGELOG.md) lists the changes in each release. Example configs: [`config.example.toml`](config.example.toml) and [`policy.example.toml`](policy.example.toml).

## API

<!-- truesight:surface summonr -->

759 public items (76 types, 105 functions, 122 methods, 267 fields, 188 variants, 1 constant) at 759 paths; 0 are re-export aliases. 394 trait impl lines, 33 inherent impl lines. 24 modules, 0 re-export only.

| Module | Items | Types | Functions | Methods | Aliases |
| --- | ---: | ---: | ---: | ---: | ---: |
| `summonr` | 512 | 42 | 56 | 99 | 0 |
| `summonr::acp` | 1 | 0 | 1 | 0 | 0 |
| `summonr::askpass` | 5 | 1 | 3 | 0 | 0 |
| `summonr::catalog` | 24 | 2 | 5 | 1 | 0 |
| `summonr::child` | 5 | 1 | 1 | 1 | 0 |
| `summonr::classify` | 38 | 5 | 2 | 3 | 0 |
| `summonr::classify::digest` | 1 | 0 | 1 | 0 | 0 |
| `summonr::classify::eval` | 20 | 3 | 4 | 1 | 0 |
| `summonr::classify::patch` | 6 | 1 | 3 | 0 | 0 |
| `summonr::classify::sets` | 5 | 0 | 4 | 0 | 0 |
| `summonr::classify::sweep` | 2 | 0 | 2 | 0 | 0 |
| `summonr::diff` | 20 | 3 | 5 | 3 | 0 |
| `summonr::limits` | 1 | 0 | 1 | 0 | 0 |
| `summonr::mail` | 0 | 0 | 0 | 0 | 0 |
| `summonr::mail::cli` | 1 | 0 | 1 | 0 | 0 |
| `summonr::modules` | 20 | 3 | 2 | 5 | 0 |
| `summonr::prefs` | 19 | 2 | 2 | 1 | 0 |
| `summonr::secret` | 10 | 1 | 2 | 1 | 0 |
| `summonr::splash` | 14 | 1 | 1 | 2 | 0 |
| `summonr::steps` | 16 | 3 | 3 | 1 | 0 |
| `summonr::supervise` | 3 | 1 | 0 | 0 | 0 |
| `summonr::tui` | 23 | 3 | 3 | 3 | 0 |
| `summonr::tui::hub` | 4 | 2 | 0 | 0 | 0 |
| `summonr::workflow` | 9 | 2 | 3 | 1 | 0 |

<!-- /truesight -->

## Security

Report vulnerabilities as described in [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).

summonr is built by Manaforge Technologies LLC.
