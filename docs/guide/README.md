# summonr guide

summonr is a terminal coding agent written in Rust. This book holds the task pages, the reference, and the pages for people who change summonr itself.

## Reading order

1. [Quick start](quickstart.md): install, sign in, and run your first goal.
2. [Models and sign-in](models.md): providers, coding plans, local servers, and where keys live.
3. [Goals and checks](goals.md): checks, goals, steps, budgets, exit codes, and worktrees.
4. [Schedules and chat](automation.md): cron jobs, webhooks, Telegram, and GitHub.
5. [Using the terminal](terminal.md): panes, sessions, child sessions, mail, and memory.
6. [Extending summonr](extending.md): skills, MCP servers, hooks, CLIs, and project instructions.
7. [Safety and policy](safety.md): what runs without asking, confirm levels, and policy files.
8. The [reference](reference.md) lists every flag, slash command, config key, exit code, and environment variable.
9. The [API map](../api.md) is for agents and tools that drive summonr.
10. [Architecture](architecture.md) explains how a turn runs.

## Internals

Read these pages before you change the source.

- [Module API](internals/module-api.md): the locked list of jobs, one name per job.
- [TUI API](internals/tui.md): the shared surface inside `src/tui`.
- [Glossary](internals/glossary.md): the domain terms.

## Build the book

```sh
mdbook serve docs
```

The output goes to `docs/book/`, which Git ignores.
