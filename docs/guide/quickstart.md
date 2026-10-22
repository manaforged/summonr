# Quick start

This page takes you from nothing to a goal run that ends with your check command passing.

## 1. Install

macOS and Linux:

```sh
curl -fsSL https://summonr.dev/install.sh | sh
```

Windows:

```powershell
powershell -c "irm https://summonr.dev/install.ps1 | iex"
```

The installer puts `summonr` in `~/.local/bin`, or `%LOCALAPPDATA%\Programs\summonr\bin` on Windows. To verify the download or build from source, see the [README](../../README.md#install).

## 2. Sign in

Run `summonr` in a project directory. On the first run it opens a provider list: pick one, then paste an API key or sign in to a coding plan in the browser. Type `/login` to open the list again later.

To skip the list, set a provider's key and name the endpoint:

```sh
export ANTHROPIC_API_KEY=...
summonr anthropic
```

To use a coding plan instead of a metered key, sign in from the shell:

```sh
summonr login              # every plan and key, with its status
summonr login openai       # sign in with ChatGPT Plus or Pro, or paste a key
summonr openai
```

[Models and sign-in](models.md) covers every provider, local servers, and where keys live.

## 3. Start a session

1. Run `summonr` (or `summonr anthropic`) in your project.
2. Ask a question, such as `what does src/main.rs do?`
3. Ask for a change, such as `add a --verbose flag`. summonr edits files and runs commands.
4. To take the change back, type `/undo`. It drops the last turn and restores the files; every turn is a git checkpoint.

summonr runs commands as your account with no sandbox. Read [Safety and policy](safety.md) before you run unattended work.

## 4. Work until a check passes

Write the command that proves the work is done:

```sh
mkdir -p .summonr
echo 'check = "cargo test"' > .summonr/check.toml
```

Run a goal inside a 30-minute budget:

```sh
summonr anthropic --goal "make cargo test pass" --max-secs 1800
```

summonr works until the check exits 0 and a second model call agrees the goal is met. Exit code 0 means done, 2 means the check still fails, and 3 means it stopped short with no check or with the goal unmet.

See what the last turn changed:

```sh
summonr diff --turn last
```

## Next steps

- [Models and sign-in](models.md): providers, coding plans, and your own servers.
- [Goals and checks](goals.md): budgets, steps, worktrees, and scripting with `--json`.
- [Using the terminal](terminal.md): panes, sessions, child sessions, and memory.
- [Extending summonr](extending.md): skills, MCP servers, hooks, and CLIs.
- [Safety and policy](safety.md): what asks first and how to narrow it.
- [Reference](reference.md): every flag, command, config key, and exit code.
