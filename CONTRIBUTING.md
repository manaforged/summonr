# Contributing

## Build

You need Rust. `rust-toolchain.toml` pins the version, and rustup
downloads it on the first build. The default build needs no system
packages. `--sandbox` on Linux needs bubblewrap.

```sh
cargo build --locked
cargo run --locked -- anthropic
```

## Check

Run these before you open a pull request. CI runs the same checks for
the packages that your change touches.

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
python3 scripts/api-census.py --check
```

The tests need no network and no keys. The census must print
`api-census deny: clean`. It also counts wildcard `_ =>` match arms and
files over 400 lines, so avoid both.

## Model catalog snapshot

`src/catalog/models-dev.json` is the models.dev data summonr uses before
it has a cache of its own. Regenerate it before a release:

```sh
home=$(mktemp -d)
SUMMONR_HOME=$home cargo run --locked -- endpoints
cp "$home/models-dev.json" src/catalog/models-dev.json
```

## Shared API

[`docs/guide/internals/module-api.md`](docs/guide/internals/module-api.md)
lists one owner for each job. Before you write a helper, find the owner
on that page and call it. A new shared helper needs a row on that page
in the same change.

## Where a change goes

| Change | Files |
| --- | --- |
| Slash command | `src/command/specs.rs` (the spec row), `src/command.rs` (the `SlashCommand` variant), the handler in `src/tui/worker/command/flow.rs` |
| Model-facing tool | the owning file among `src/tools/{workspace,command,search,todo,lsp}.rs`, `src/tools.rs`, `src/tools/catalog.rs`, `src/runtime/turn/calls.rs`, and a row in `module-api.md` |
| Endpoint preset | one `Preset` in `src/endpoint/preset/head.rs` or `tail.rs` |
| Provider wire | the `Wire` enum in `src/endpoint.rs`, `src/provider/wire.rs`, `src/provider/wire/NAME.rs`, `src/provider/client.rs`, `src/provider/stream.rs` |

Update `docs/guide/reference.md` when you change a flag, a command, or
a config key.

## Code rules

- Write no comments. `// SAFETY:` on an unsafe block is the exception.
- Keep each source file at 400 lines or fewer.
- Return errors through `summonr::Error`. Do not hide them with `.ok()`
  or `let _ =`.

## Commits

Use one commit for each change. The subject is
`area: what the commit does`, in the imperative, under 50 characters,
with no period. The body has four lines, `Problem:`, `Fix:`, `Impact:`,
and `Test:`, with one sentence each. `Test:` names the command that you
ran and the output line that proves the change.
