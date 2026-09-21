# summonr guide

summonr is a terminal coding agent written in Rust. This book holds the
task pages, the reference, and the pages for people who change summonr
itself.

## Reading order

1. The [README](../../README.md) covers requirements, install, and the
   first session.
2. [Extending summonr](extending.md) shows how to add a skill, an MCP
   server, a hook, or a CLI.
3. The [reference](reference.md) lists every flag, slash command, config
   key, exit code, and environment variable.
4. The [API map](../api.md) is for agents and tools that drive summonr.
5. [Architecture](architecture.md) explains how a turn runs.

## Internals

Read these pages before you change the source.

- [Module API](internals/module-api.md): the locked list of jobs, one
  name per job.
- [TUI API](internals/tui.md): the shared surface inside `src/tui`.
- [Glossary](internals/glossary.md): the domain terms.

## Build the book

```sh
mdbook serve docs
```

The output goes to `docs/book/`, which Git ignores.
