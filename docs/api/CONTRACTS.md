# Contract map

Status: complete

The API that is not Rust: process, config, wire, disk. Rust items are in
the local `callable-inventory.md`. Every count here was
extracted from the tree at `f604d7be`; the source of each list is named.

## Process

Binary: `summonr`. `summon` and `summoner` are PATH aliases of the same
binary (`scripts/summon-dev`).

Flags parsed in `src/main/cli.rs`, 34 total:

```
--add-dir --continue --effort --endpoint --fork --goal --gui --help --here
--json --key --max-secs --max-tokens --max-turns --model --name --net
--no-net --no-sandbox --no-splash --no-trust --no-verify --plan --print
--sandbox --session --stat --steps --strict --trust --turn --version
--workflow --worktree
```

Modes: one-shot turn, `--goal`, `--steps`, `--plan` (read-only), `--gui`
(build feature), `--worktree`.

Exit codes: 0 pass, 1 usage or internal failure (`src/main.rs:18-37`),
2 red check, 3 budget breach, stalled goal, or aborted turn
(`src/main/modes.rs:174`).

## Slash commands

46 names, from `spec` rows in `src/command.rs`:

```
/agents /approve /auth /auto /branch /bus /clear /compact /copy /diff
/effort /exit /export /fork /goal /help /history /layout /login /model
/modules /name /net /pane /plan /quit /reasoning /receipts /resume /retry
/profile /rollback /search /session /settings /stats /steps /theme /tools /tree
/undo /view /workflow /write
```

Subcommands include `summonr acp` (Agent Client Protocol v1 over stdio) and
`summonr profile [ID|NAME]` (bottleneck report from a session journal).

## Tools

Nine schemas. Eight in `src/tools/catalog.rs` — `read`, `edit`, `write`,
`todo`, `skill`, `lsp`, `run_command`, `web`. One in
`src/runtime/child.rs:95` — `task`.

Gating: `task` is off until `/modules task on`; plan mode advertises `read`,
`run_command`, `skill`, and `web` only. Reads return `N:hash|text` lines and
edits that miss a hash fail before write.

## Endpoints

22 built-in rows (`README.md`): `zai`, `zai-cn`, `glm`, `glm-intl`, `qwen`,
`kimi`, `grok`, `copilot`, `openai`, `openrouter`, `together`, `groq`,
`cerebras`, `mistral`, `muse`, `anthropic`, `gemini`, `zen`, `go`,
`opencode`, `jev`, `typesafe`.

Wires: `chat/completions` (most), `responses` (selectable), Anthropic
Messages, Gemini, `decisions`. `together`, `groq`, `cerebras`, and
`mistral` set no default model.

A `decisions` endpoint answers typed questions instead of streaming text.
Its `base_url` is the whole URL it posts to, and `summonr infer` picks
the first one that has a key. Any endpoint declares itself one with
`wire = "decisions"` in `~/.mage/config.toml`, so a new model of this
type needs no code change:

```toml
[endpoint.my-judge]
base_url = "https://provider.example/v1/decide"
model = "their-model-name"
wire = "decisions"
```

Chat pickers skip these rows, so a decisions endpoint is never offered
as a chat provider.

A question is a Choice, a Score or a Noul. A Choice takes 1 to 255
options as a map of name to description and answers with the option, a
probability for each, and a confidence. A Score takes 2 to 10 ordered
levels and answers with a probability weighted position between them,
the legend, and a confidence. A Noul answers with the probability that
one statement is true and carries no confidence, because the probability
is the certainty. Instructions, option descriptions, score levels and a
Noul boundary each take a string, an object or an array, so options that
are easily confused can name what they are for and what they are not.
State takes a string, an object or an array as well.

Question sets ship by name: `errors` classifies one failure, `diff`
asks about one file of a patch, `commit` asks about a whole patch, and
`review` scores a session. Granularity is part of the question: asking
`untested` per file answers no every time, because a production file
never holds its own test, while the same question over a whole patch
scores every example right.

A question set is scored against labelled examples, one JSON object per
line with `state` and `expect`. `scripts/diff-corpus.py` builds such a
corpus from a repository's own history, writing it outside the tree,
because the examples hold verbatim source. The score separates plain
accuracy from accuracy above the confidence gate, and the gated number
is the one that decides whether a set is usable. Repeat runs vary by
about two points, so read a smaller difference as noise.

State larger than the context is split into windows on line boundaries,
every window is asked in parallel, and the answers merge: the highest
probability wins a Noul, the worst level wins a Score, and Choice
distributions blend weighted by confidence. Nothing is truncated, so a
failure in the middle of a long transcript still reaches the answer.

## Environment

`MAGE_*` (13): `MAGE_API_KEY`, `MAGE_BASE_URL`, `MAGE_BUS`, `MAGE_BUS_ID`,
`MAGE_BUS_KIND`, `MAGE_CODEX`, `MAGE_ENDPOINT`, `MAGE_HOME`, `MAGE_MODEL`,
`MAGE_ROOM_LOG`, `MAGE_ROOM_PUSH`, `MAGE_SECRETS`, `MAGE_SYSTEM_PROMPT`.

`SUMMONR_*` (5): `SUMMONR_EDITOR_FILE`, `SUMMONR_REVISION`,
`SUMMONR_SANDBOX_DETACHED`, `SUMMONR_SANDBOX_DIRECTORY`,
`SUMMONR_SANDBOX_ROOT`.

Provider keys read from the environment (16): `ANTHROPIC_API_KEY`,
`CEREBRAS_API_KEY`, `GEMINI_API_KEY`, `GOOGLE_API_KEY`, `GROQ_API_KEY`,
`MISTRAL_API_KEY`, `MOONSHOT_API_KEY`, `OPENAI_API_KEY`, `OPENCODE_API_KEY`,
`OPENCODE_GO_API_KEY`, `OPENROUTER_API_KEY`, `QWEN_LOCAL`, `TOGETHER_API_KEY`,
`XAI_API_KEY`, `ZAI_API_KEY`, `ZAI_CODING_API_KEY`, `ZAI_CODING_CN_API_KEY`.

Keys otherwise live in `~/.mage/credentials.toml`, a ChaCha20-Poly1305 blob
whose wrapping key is in the OS keychain.

## Config keys

`~/.mage/config.toml` (a workspace `.mage/config.toml` may select an `[lsp]`
module only). From `config.example.toml`:

- top level: `default`, `theme`, `splash`, `fps`, `reasoning`, `tools`, `bus`,
  `workflow`
- `[endpoint.NAME]`: `base_url`, `model`, `api_key`, `context_window`,
  `compact_tokens`, `fallback`, `effort`, `price`, `wire`, `[endpoint.NAME.body]` (merged into each request),
  `[endpoint.NAME.env]`, `[endpoint.NAME.auth]` (`token_url`, `device_url`,
  `client_id`, `client_secret`, `scope`, `grant`, `exchange_url`)
- `[lsp]`: `module`, `command`
- `[modules]`: `task`, `mcp`
- `[mcp.NAME]`: `command`, `args`, `env`, `url`, `headers`,
  `bearer_token_env_var`, `enabled`, `auth` (the endpoint `auth` keys).
  Servers are also read from `~/.codex/config.toml`, `~/.claude.json`, and
  `.mcp.json`; project servers load only when approved
- `[hooks]`: `pre_tool`, `post_tool`, `compact`, `stop` (shell commands,
  JSON on stdin)
- `[sandbox]`: `write`
- `[search]`: `backend`, `api_key`, `count`, `allowed_domains`,
  `excluded_domains`
- `[workflows.NAME]`: `scout`, `plan`, `implement`, `review`, `judge`,
  `summary`

## Policy keys

`~/.mage/policy.toml` and `<workspace>/.mage/policy.toml`. Project policy can
narrow user permissions and never widen them. Keys: `strict`, `deny`, `allow`,
`write`. In the default mode commands run uncontained and `policy/danger.rs`
holds destructive ones (recursive delete, `git reset --hard`, force push,
protected paths, and the built-in denies) for approval each time. With the
sandbox on, built-in denies apply and `rm`, `mv`, `cp`, `chmod`, `tee`, and
redirects may not name a path outside the workspace.

## Disk

| Path | Contents |
|---|---|
| `~/.mage/config.toml` | endpoint rows, interface prefs, `[sandbox] write` |
| `~/.mage/policy.toml` | strict, allow, deny, write |
| `~/.mage/credentials.toml` | sealed key blob |
| `~/.mage/sessions/<id>/session.jsonl` | append-only conversation log |
| `~/.mage/sessions/<id>/work.json` | the job, rewritten atomically per transition |
| `.mage/check.toml` | the commands that decide pass or fail |
| `.mage/plan.md` | the plan pane's file |
| `.mage/skills/` | the `skill` catalog |
| bus `inbox` / `board` | agent room mail, 8 and 6 references in `src` |

Writes to these files go through 18 direct `fs::write` or `File::create`
sites (see the local audit, S5).

## Room protocol

`src/rpc.rs` speaks JSON-RPC over stdio with `protocolVersion`
`2025-03-26` (an MCP protocol revision). Three users share it: the room
server (`src/room/serve.rs`), the MCP client (`src/mcp.rs`, stdio and
Streamable HTTP), and the ACP server (`src/acp.rs`).
