# Footprint gate

Measures summonr memory and child processes against a scripted model and
fails when a build grows past thresholds captured from `main`.

## Scenarios

Defined in `scenarios.toml`. Bands per metric live in `[bands]`. A `match` gate may add `allow_kb` on top
of its reference.

| Scenario | Proves |
| --- | --- |
| `a-off` | Baseline peak with the MCP module off. |
| `a-on` | Turning the MCP module on with no servers costs nothing: peak matches `a-off`. |
| `b-warm` | With three servers and a warm tool catalog, a turn that calls no MCP tool starts no server process. |
| `b-cold` | With a fresh home, any servers started to build the catalog are gone by the end; start count is recorded. |
| `c-idle` | An idle server (`idle_secs = 2`) stops while the model takes 6 s to reply; at most one server runs. |
| `d-big` | A 10 MiB tool result is dropped at the 4 MiB reply cap; the peak stays within `a-on` plus 8 MiB, twice the cap. |
| `f-accept` | A 3 MiB tool result is accepted; the peak stays within `a-on` plus 12 MiB, four times the reply: the raw line, the JSON parser's unescape buffer, and the parsed text coexist while the reply is parsed. |
| `e-long` | 200 MCP calls in one session do not grow the peak. |

Metrics per run: `footprint_kb` (max `footprint_kb` over the session log close
lines), `parent_peak_kb`, `tree_peak_kb`, `children_peak`, `children_tail`
(max children in the final `tail_fraction` of samples), `starts` (fixture
process starts). The gate takes the median over runs.

## Build

```sh
cargo build --release
fixture=$(cargo test --release --locked --test mcp --no-run 2>&1 \
  | sed -n 's/.*Executable tests\/mcp.rs (\(.*\))/\1/p')
```

The MCP test binary is the fixture server: it serves MCP on stdio when
`MCP_FIXTURE_SERVE` is set, and the gate sets it.

## Run

Capture thresholds on `main`. This writes `thresholds-<platform>.toml` here
(`darwin` or `linux`, from Python `sys.platform`); each platform keeps its own
numbers because macOS reports physical footprint and Linux reports max RSS:

```sh
bench/footprint/run.sh capture target/release/summonr "$fixture" target/footprint-main
```

Check a branch build. It prints `PASS|FAIL scenario metric value threshold`
and exits 1 on any failure:

```sh
bench/footprint/run.sh check target/release/summonr "$fixture" target/footprint-branch
```

Name scenarios after the output directory to run a subset. Both commands
write `OUT_DIR/results.json`.
