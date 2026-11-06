# Contributing

Issues are welcome. Pull requests are by invitation: open an issue first, and send a patch only when asked.

## Build

You need Rust 1.98. `rust-toolchain.toml` pins the version, and rustup downloads it on the first build. The default build also needs Git, CMake, and a C and C++ compiler.

```sh
cargo build --locked
cargo run --locked -- anthropic
```

`scripts/summon-dev` runs a release build that it rebuilds when the source changes. With a Sigil checkout at `../sigil`, it builds against that checkout and puts `Cargo.lock` back afterwards. Link it onto your `PATH` as `summonr` to work on summonr and Sigil together.

## Check

Run these before you send a patch. The CI workflow checks formatting, the API census, the palette, dependency policy, Clippy, and tests on Linux, plus tests on macOS and Windows. Its `required` job succeeds only when all three platform jobs pass.

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
python3 scripts/api-census.py --check
python3 scripts/palette-check.py
cargo deny check
```

The tests need no network and no keys. The census must print `api-census deny: clean`. It also counts wildcard `_ =>` match arms and files over 400 lines, so avoid both.

## Model catalog snapshot

`crates/agent/src/catalog/models-dev.json` is the models.dev data summonr uses before it has a cache of its own. Regenerate it before a release:

```sh
home=$(mktemp -d)
SUMMONR_HOME=$home cargo run --locked -- endpoints
cp "$home/models-dev.json" crates/agent/src/catalog/models-dev.json
```

## Shared API

[`docs/guide/internals/module-api.md`](docs/guide/internals/module-api.md) lists one owner for each job. Before you write a helper, find the owner on that page and call it. A new shared helper needs a row on that page in the same change.

## Where a change goes

| Change | Files |
| --- | --- |
| Slash command | `crates/agent/src/command/commands.toml` (the spec row), `crates/agent/src/command.rs` (the `SlashCommand` variant), the handler in `crates/ui/src/worker/command/flow.rs` |
| Model-facing tool | the owning file among `crates/agent/src/tools/{workspace,command,search,todo,lsp}.rs`, `crates/agent/src/tools.rs`, `crates/agent/src/tools/catalog.rs`, `crates/agent/src/runtime/turn/calls.rs`, and a row in `module-api.md` |
| Endpoint preset | one `Preset` in `crates/agent/src/endpoint/preset/head.rs` or `tail.rs` |
| Provider wire | the `Wire` enum in `crates/agent/src/endpoint.rs`, `crates/agent/src/provider/wire.rs`, `crates/agent/src/provider/wire/NAME.rs`, `crates/agent/src/provider/client.rs`, `crates/agent/src/provider/stream.rs` |

Update `docs/guide/reference.md` when you change a flag, a command, or a config key.

## Code rules

- Write no comments. `// SAFETY:` on an unsafe block is the exception.
- Keep each source file at 400 lines or fewer.
- Return errors through `summonr::Error`. Do not hide them with `.ok()` or `let _ =`.

## Commits

Use one commit for each change. The subject is `area: what the commit does`, in the imperative, under 50 characters, with no period. The body has four lines, `Problem:`, `Fix:`, `Impact:`, and `Test:`, with one sentence each. `Test:` names the command that you ran and the output line that proves the change.
