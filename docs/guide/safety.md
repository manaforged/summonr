# Safety and policy

This page explains what summonr can touch, what it always protects, what asks before it runs, and how to narrow it with policy files.

## What summonr can touch

summonr has no sandbox. Shell commands, file tools, MCP servers, and language servers run as your account, with its files and network, and most commands run without a prompt. Run summonr in a container when a session should not touch the rest of the machine.

### The environment a command gets

`run_command` does not pass your whole environment to the shell. A command gets `PATH`, `HOME`, `SHELL`, `USER`, `LOGNAME`, `TERM`, `LANG`, `LC_ALL`, the temp directory variables, `CARGO_HOME`, `RUSTUP_HOME`, `CARGO_TARGET_DIR`, and the Windows system variables, and `SSH_AUTH_SOCK`, so `git` over SSH reaches your agent, plus the password helpers for `sudo` and `ssh`. Anything else, such as `DATABASE_URL`, `JAVA_HOME`, `AWS_PROFILE`, `HTTPS_PROXY`, or `GH_TOKEN`, is not set.

To give commands a variable, add an `env` table to the endpoint you run:

```toml
[endpoint.anthropic]
env = { DATABASE_URL = "postgres://localhost/dev" }
```

## What always holds

Two protections hold in every session, with or without `--strict`.

`search` never reads a protected file or folder.

Secret values never reach the model. Before a tool result is shown or sent, summonr replaces every value that looks like a secret with `[secret]`. It checks these sources:

- `.env` and `.env.*` files in the workspace, up to three folders deep
- `.npmrc` and `.pypirc` in the workspace
- `~/.aws/credentials`, `~/.netrc`, `~/.npmrc`, `~/.pypirc`, `~/.git-credentials`, `~/.config/gh/hosts.yml`, and `~/.docker/config.json`
- Every PEM private key block

A value looks like a secret when its key names one (`KEY`, `SECRET`, `TOKEN`, `PASSWORD`, `AUTH`, and similar words, in `KEY=value`, `key: value`, or JSON form), when it is a URL with a password, or when it is a long mix of letters and digits. Commands still read the real files, so a test suite that loads `.env` runs as before.

`sudo` asks for your password in a dialog. The password goes to `sudo` through `SUDO_ASKPASS`; the model never sees it, and command output is scrubbed of it.

## What asks first

Irreversible destruction asks before it runs in every session: wiping the system, home, or project root, `mkfs`, `dd` to a device, a fork bomb, `chmod -R 777 /`, `terraform destroy`, cluster deletes, and dropped database tables. The prompt shows the whole command, wrapped, with the reason as `<command> — <reason>`. You answer with `[ allow ]` / `[ deny ]`, or `y` / `n`. `-p` has no one to ask, so those commands do not run there.

Each turn's checkpoint makes the rest recoverable with `/undo`, for example `git reset --hard`.

`confirm` in `policy.toml` picks which commands ask first:

| Value | Asks before |
| --- | --- |
| `confirm = "destructive"` (default) | Irreversible destruction only |
| `confirm = "risky"` | Irreversible destruction, `sudo`, force pushes, pipes into a shell, git commands that discard work or rewrite history, and writes to protected paths |

```toml
confirm = "risky"
```

The stricter value wins across overlays. Shift+Tab toggles the session between auto approval and approval for risky commands. Auto approval still asks before destructive commands and configured `ask` patterns. The toggle cannot lower a configured `confirm = "risky"`, and it cannot relax strict mode, command denies, or write restrictions.

The composer labels approval separately from the auto, work, and goal execution modes. A toggle during a turn is queued until the turn ends, and the active label stays unchanged until then.

`ask` lists commands that always ask first, whatever `confirm` says. A command matches when it contains a pattern, the same rule as `deny`. A match shows `[ allow ]` / `[ deny ]`, and under `-p` it does not run. Strict mode does not read `ask`.

```toml
ask = ["git push", "git tag", "cargo publish", "npm publish", "gh release"]
```

## Strict mode

`--strict` turns on the command and file policy.

### Reads

`read` and `search` reach any path except the protected ones. The summonr home is protected wherever `SUMMONR_HOME` puts it. A command that reads a protected path is refused from any `workdir` or after `cd`. These reads are blocked:

- `.env` and `.env.*`, files named `credentials` or `credentials.*`, `.netrc`, `.git-credentials`, and `.pypirc`
- The home folders `.ssh`, `.gnupg`, `.aws`, `.summonr`, `.docker`, `.kube`, `.config/gh`, and `.config/gcloud`
- A shell content search such as `grep -r` or `rg`, or a copy, over a folder that holds one of those directories

A recursive shell search of a folder that holds a `.env` still runs, and the secret values in what it prints are replaced.

### Writes

Under `--strict`, `edit` and `write` stay inside the writable roots: the launch directory, `write_roots` in `~/.summonr/config.toml`, and `--add-dir`.

```toml
write_roots = ["~/.cargo/registry", "~/code"]
```

A `~/` prefix expands to your home directory. A listed path that does not exist is skipped at startup, and it is still a writable root for policy checks. For work that spans sibling repositories, put the parent on the list, launch summonr in the parent directory, or pass `--add-dir`.

Auto mode asks before it grants a write outside the writable roots, then retries the tool. Without an interactive channel, the write stays denied.

| Command | Effect |
| --- | --- |
| `/permissions write PATH` | Adds the path to `write_roots` without a prompt |
| `/permissions write --session PATH` | Grants the path for this session only |
| `/permissions write` | Lists the extra roots |

`~` and `$HOME` expand before the check. A write to a protected path asks before it is granted: `.git/hooks`, `.git/config`, `.git/HEAD`, `.git/index`, `.git/refs`, `.git/logs`, `~/.ssh`, `~/.gnupg`, `~/.summonr`, shell startup files, and `.env`. A protected file inside the workspace stays denied.

### Commands

Under `--strict`, irreversible commands never run, and neither do force pushes, `sudo`, pipes into a shell, history rewrites, or writes to protected paths. Matches on your own `[policy] deny` list are refused without a prompt. With `write` globs in `policy.toml`, shell commands do not run; file tools still use the globs.

In a read-only research child, `edit`, `write`, and `run_command` are unavailable.

## Policy files

User policy is `~/.summonr/policy.toml`. Repository policy is `.summonr/policy.toml`. A repository policy can narrow your permissions but never widen them, and child sessions inherit the restrictions.

| Setting | How overlays combine |
| --- | --- |
| Command allow list | A nonempty list must match on every overlay (intersection) |
| `deny` | Lists accumulate |
| `ask` | Lists accumulate. A repository can add patterns, never remove them. |
| `confirm` | The stricter value wins |
| `write` globs | A narrowing filter on paths relative to a writable root |

Extra writable roots come only from `write_roots`, `--add-dir`, and `/write`, never from `write` globs. Empty lists add no restriction, except that `strict = true` with an empty allow list denies all commands.

MCP tool aliases in deny lists are covered in [Extending summonr](extending.md#add-an-mcp-server). Retry and timeout limits are in the [reference](reference.md).

## summonr does not steer the model

Tool results reach the model unchanged, and the model decides when a turn is done; only a token or wall-clock budget that you set ends a turn early. A reply cut off at the output limit gets one note saying so. The compaction summary request sends the session's tools and history unchanged, so it reuses the prompt cache, and a failed summary retries after 8 rounds.
