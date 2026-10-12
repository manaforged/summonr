# summonr reference

This page documents the flags, config keys, environment variables, exit codes, slash commands, panes, policy rules, workflow roles, child sessions, and peer mail. The [README](../../README.md) is the short form.


## Usage

```sh
summonr grok
summonr anthropic
summonr kimi -p "What is in Cargo.toml?"
```

A single word that is not a command, endpoint, or prompt flag but looks like a mistyped command (`summonr sesions`) is refused with a suggestion; use `-p` to send it as a prompt.

summonr starts on the first endpoint and model that applies: the endpoint and `--model` flags, then `SUMMONR_ENDPOINT` and `SUMMONR_MODEL`, then the default (`default` and `default_model` in `config.toml`), then the last endpoint and model you used, then the first endpoint that is set up. The last-used seat lives in `~/.summonr/recent.json` and changes only after a turn succeeds. A `default` or last-used endpoint that no longer exists or has no key prints one note, and summonr falls back to the next rule. Only an endpoint named on the command line is a hard error. `alt+d` in the `/model` picker sets the selected row as the default, and `alt+d` on the default row clears it. The header shows which rule chose the endpoint, for example `(default)` or `(last used)`, until you switch.

Add your own OpenAI-compatible server from the command line:

```sh
summonr login local --url http://127.0.0.1:8080/v1
summonr logout local --remove
```

`summonr login NAME --url URL` adds the endpoint. When the server lists exactly one model, summonr picks it; otherwise pass `--model NAME`. The same command with an existing name updates that endpoint in place. `summonr login NAME --model M` changes the model of an endpoint you added without `--url`; on a built-in endpoint, `--model` needs `--default`. `summonr login NAME --default` sets `default` and `default_model` for any endpoint that is set up; it signs in first when the endpoint is not set up, and `--model M` picks the model it saves. `summonr login NAME --help` prints the help for `login`; `logout`, `diff`, `profile`, `install`, `export`, `doctor`, `classify`, `worktrees`, and `mcp` do the same. A built-in name is refused. `summonr logout NAME --remove` deletes the endpoint and forgets its key; when `default` named it, `default` and `default_model` are cleared. An unknown endpoint name suggests the closest name, or `summonr login NAME --url URL`.

Or add the host in `~/.summonr/config.toml`:

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8080/v1"
model = "whatever-the-server-calls-it"
```

Then `summonr local`. A local server needs no `api_key`. When a local built-in such as `ollama` or `lmstudio` is not running, the error says so and names the preset's `start` command (`ollama serve`, `lms server start`). With no model set, the hint lists the models the server has. `effort = "low"` on an endpoint (or `--effort LEVEL`, `/effort` in the interface) sets the reasoning effort for that endpoint. `/effort` and `tab` are remembered per endpoint. An endpoint with no effort set, and none remembered, uses `high` when the model catalog says the model reasons, and sends no effort otherwise. Reasoning effort below lists the levels each model accepts. `price = { input, output, cached, cache_write }` in USD per million tokens (`cache_write` defaults to `input`) prices every round in the footer. A workflow role keeps its own endpoint's effort and price; `--effort` and `/effort` set the driving endpoint and any role whose endpoint does not set them. `summonr endpoints` lists the endpoints that are set up first, and the models of each name through `GET /models`; a host that serves its model list away from its chat base sets `models_url` (`perplexity` does: chat at `/chat/completions`, models at `/v1/models`). Each row shows the key state: `ready`, `needs key`, `sign in`, `key rejected`, `rate limited`, `offline`, `error`, or `no key needed`. `summonr login NAME` prompts for an API key. On an endpoint that needs no key, it says so and reports the endpoint ready, or names the missing model. `summonr login` prints one line per endpoint with its state and label, and `summonr endpoints` one row per endpoint; both size each column to its longest value, and a row with no model shows only its URL; endpoints that are set up come first in each group. A keyless built-in with no model reads `needs model`. `grant = "device_code"` runs a device login, and `grant = "authorization_code"` runs a browser sign-in with PKCE (S256).

`summonr endpoints` and the `/model` picker download the [models.dev](https://models.dev) catalog (`https://models.dev/api.json`, about 350 KB) to fill in model names, context windows, reasoning and vision support, and prices. They fetch it even when every endpoint is local. The copy is cached in `~/.summonr/models-dev.json` and fetched again at most once every `models_catalog_secs` (an hour), with `If-None-Match`. `-p` runs and interface start do not fetch it. Without a cached copy, summonr uses a snapshot built into the binary. `SUMMONR_MODELS_URL` sends the fetch to another URL, such as a mirror you host, and `models_catalog = false` in `config.toml` turns the fetch off.

The browser flow picks its return path from `redirect_uri`:

- A loopback `redirect_uri`, or none at all, opens a local port, catches the provider's redirect, and closes. Omit it and summonr picks a free port; set it when the provider registered a fixed one. Port `0` keeps the provider's path and lets summonr pick the port.
- When the redirect carries a `client_id`, summonr stores it with the token and uses it for every refresh and the next sign-in. This is how a provider that registers the app on first sign-in hands out its id.
- Any other `redirect_uri` switches to paste-back: summonr asks for the code the browser shows and accepts the `code#state` form. No port opens.

Endpoints carry the rest of what a provider needs, so a new provider is config, not code:

- `token_style = "json"` when the token endpoint takes a JSON body instead of a form.
- `scheme` sets the carrier for the token: `bearer` (the default for every login), `api_key`, or `google_key`.
- `pkce = true` adds a PKCE challenge to a device login.
- `[…auth.authorize_params]` and `[…auth.token_params]` add provider parameters to the two requests.
- `[…auth.claim_headers]` maps a header name to a claim read from the token, so an account id travels on every request.

`config.example.toml` carries ready endpoints for other subscriptions. The `claude-pro` endpoint is off: Anthropic's terms do not allow a third-party app to sign in with a Claude Pro or Max subscription.

An endpoint can offer both a sign-in and a key. `summonr login NAME` then asks which one; `--key` skips the question. A stored sign-in wins over any key, and `summonr logout NAME` returns the endpoint to its key. `auth.label` names the subscription in that question.

`openai` offers Sign in with ChatGPT (Plus or Pro) next to `OPENAI_API_KEY`, and runs on the subscription's usage through the Responses API. The first sign-in registers summonr with OpenAI and stores the issued client id; `~/.summonr/host-id` holds the install id that OpenAI asks for on every sign-in. OpenAI caps each app per week in ChatGPT settings → Usage; when the cap is reached, the error names that page after the standard retry policy. The subscription path rejects `max_output_tokens`, sampling fields, and hosted tools; on a subscription token summonr sends no `prompt_cache_key` and groups its tools in one `namespace`, as that route requires. A sign-in that does not grant `chatgpt.tokens.use.direct` is refused, so the endpoint never shows signed in without subscription usage.

`grok` offers a SuperGrok or X Premium+ sign-in next to `XAI_API_KEY`, through the device flow at `auth.x.ai`.

Keys live in `~/.summonr/credentials.toml`, a plain TOML file with mode 0600, written through a rename. Only your account can read it. A sealed file from an earlier build is decrypted with its `credentials.wrap` key and rewritten as plain TOML the first time summonr reads it; after that, `credentials.wrap` is no longer used. The key prompt disables echo and stores the key; `summonr login NAME --key` skips the method question on a row that offers both, and storing a key signs that row out of its plan. `summonr logout NAME` forgets the key and any login token, says what it removed (`Signed out of NAME` for a sign-in token, `Removed the stored key for NAME` for a key, or that nothing was stored; on an endpoint that needs no key, it says nothing was stored), and names the key that applies next. A plaintext `api_key` in `config.toml` is moved into that file the first time summonr runs, and it says so once. Resolution order is the host's environment variable, the store, peer stores, `api_key` in `config.toml`, then the built-in default. An endpoint with no key fails with the missing-key message; only local and keyless endpoints run without one. `--key` is refused on a row that only signs in, such as `copilot`.

Peer stores are read live: OpenCode's `~/.local/share/opencode/auth.json` lends `type: "api"` keys, plus unexpired `type: "oauth"` access tokens on bearer endpoints; Kimi's `~/.kimi-code/credentials/kimi-code.json` lends its unexpired token. A peer key reports as `key · opencode` or `key · kimi-code` in `summonr login`. A stored `summonr login NAME` key wins over a peer key; `summonr logout NAME` returns the endpoint to the peer. Configured endpoints whose `base_url` host matches a built-in share that built-in's peer mapping.

If you want the OS store instead, set `secrets = "keychain"` in `config.toml` (or `SUMMONR_SECRETS=keychain`). On macOS that is one Keychain entry holding every secret, read and written through `/usr/bin/security`, so the one-time "Always Allow" is granted to Apple's signed tool and survives every rebuild of summonr; Windows uses Credential Manager and Linux the Secret Service, one entry per secret. A locked keychain is reported, never worked around with a file, and the two modes never fall back to each other.

Built-ins: chat/completions endpoints `zai`, `zai-cn`, `glm`, `glm-intl`, `qwen-plan`, `qwen-plan-cn`, `dashscope`, `dashscope-cn`, `deepseek`, `kimi`, `grok`, `copilot`, `openrouter`, `together`, `groq`, `cerebras`, `mistral`, `muse`, `zen`, `go`, `opencode`, `fireworks`, `deepinfra`, `siliconflow`, `siliconflow-cn`, `nvidia`, `huggingface`, `cohere`, `perplexity`, `ollama`, `lmstudio`, `ollama-cloud`, `minimax`, `minimax-cn`, `tencent-plan`, `volcengine-plan`; the Responses endpoint `openai`; the Anthropic Messages endpoints `anthropic`, `minimax-plan`, and `minimax-cn-plan` (the MiniMax coding plan); and `gemini`. The whole list is one data table, `src/endpoint/presets.toml`: a provider is a `[[preset]]` row with its base URL, env keys, wire, and reasoning quirks, and a row summonr cannot honor fails startup by name. The coding-plan rows (`qwen-plan`, `qwen-plan-cn`, `minimax-plan`, `minimax-cn-plan`, `tencent-plan`, `volcengine-plan`, plus the z.ai, Kimi, and MiniMax hosts) take the key their vendor's console issues for the plan. Rows on the same plan read each other's stored key through `key_from`: `zai` and `glm-intl`, `zai-cn` and `glm`. A row's `label` names its region, its default model, and the row whose key it shares in `summonr login`. No classify endpoint ships: add an `[endpoint.NAME]` row with `wire = "decisions"` to use `summonr classify`. OpenRouter is a chat endpoint (`OPENROUTER_API_KEY`); `/model` lists its catalog the same way it lists OpenCode. `together`, `groq`, `cerebras`, `mistral`, `fireworks`, `deepinfra`, `nvidia`, `huggingface`, `siliconflow`, `siliconflow-cn`, `ollama-cloud`, `ollama`, and `lmstudio` carry no default model: pass `--model`. `deepseek` defaults to `deepseek-v4-flash`, `dashscope` to `qwen3-coder-plus`, `cohere` to `north-mini-code-1-0`, and `perplexity` to `sonar-pro`. `summonr login copilot` is GitHub device login; chat then exchanges that token for a Copilot session token. Kimi is a key (`KIMI_CODING_API_KEY`, `KIMI_API_KEY`, or `MOONSHOT_API_KEY`) or a token in `~/.kimi-code/credentials/kimi-code.json`. Environment keys are the host's own (`XAI_API_KEY`, `ZAI_API_KEY` first for `zai`, `ZAI_CODING_CN_API_KEY` first for `zai-cn`, `GLM_CODING_API_KEY` first for `glm`, `ZAI_CODING_API_KEY` first for `glm-intl`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`, `TOGETHER_API_KEY`, `GROQ_API_KEY`, `CEREBRAS_API_KEY`, `MISTRAL_API_KEY`, `META_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`, `OPENCODE_API_KEY`, `OPENCODE_GO_API_KEY`, `DEEPSEEK_API_KEY`, `FIREWORKS_API_KEY`, `DEEPINFRA_API_KEY`, `MINIMAX_API_KEY`, `MINIMAX_CN_API_KEY`, `ALIBABA_CODING_PLAN_API_KEY`, `DASHSCOPE_API_KEY`, `TENCENT_CODING_PLAN_API_KEY`, `ARK_CODING_PLAN_API_KEY`, `SILICONFLOW_API_KEY`, `SILICONFLOW_CN_API_KEY`, `NVIDIA_API_KEY`, `HF_TOKEN`, `COHERE_API_KEY`, `PERPLEXITY_API_KEY`, `OLLAMA_API_KEY`). Overlay `wire = "anthropic"` on any `[endpoint.NAME]` row. See [`config.example.toml`](../../config.example.toml).

```sh
summonr --goal "make cargo test pass"
summonr --steps PLAN.md
```

`summonr --name TEXT` (or `/name TEXT`) names a session. Every place that shows a session id shows the short id: leading zeros dropped, 12 characters kept. `--session` takes a name, the short id, or an id prefix; `summonr sessions` lists both (with a header row on a terminal), and `summonr export [ID|NAME]` prints the transcript as Markdown. `summonr doctor --logs` lists the last 20 per-run diagnostic logs. `summonr doctor` prints the running binary path and reports crashed or incomplete runs. Logs older than 14 days are removed unless their process is still active. Session directories under `~/.summonr/sessions/` with no file change for 30 days are removed once per start, except the session that run opens. Set another period in days, 1 or more, in `~/.summonr/config.toml`:

```toml
[sessions]
retain_days = 90
```

`--goal` keeps one objective until `.summonr/check.toml` is green and a judge says stop. The judge answers `done`, `continue`, `stalled`, or `impossible`; the last two end the goal with the reason. A goal has no turn cap; it runs until the judge stops it, a budget ends it, or you cancel it. `--max-tokens` and `--max-secs` bound every mode. A budget counts from the run that loads the session, so a resumed or forked session starts with its full budget. Every goal turn restates the objective to the model, so a compacted context cannot lose it. `--steps` hands the model one numbered item at a time. A step that contains ``Run `CMD` `` gets `CMD` as its own check, in addition to `.summonr/check.toml`. A step with no `Run` phrase is gated on `.summonr/check.toml` alone. The last step runs every check. A turn that ends while a background command it started is still running is deferred, with no verdict and no repair strike, until the job exits or the model kills it. The harness remembers that job across turns; a later turn cannot complete while it runs. A `### Heading` under `## Steps` groups the items that follow it into a phase. Every turn carries the ledger to the model and to the transcript: phases as Roman numerals, `[x]` done, `[/]` this step, `[ ]` later, `[!]` blocked. Both edit this directory. `--worktree` isolates in a git worktree (`<repo>-worktrees/<id>`). Exit 0 then commits the files the run changed as `summonr: <objective>` (files over 5 MB and anything the run did not touch are left in place and named), fast-forwards your branch, and removes the tree; a tree that cannot fast-forward or still holds uncommitted files is kept and named, and the exit code is 1. `--here` is the default.

```markdown
## Steps

1. Add the `--limit` flag. Run `cargo test cli`.
2. Document the flag in README.md.
```

### Flags

| Flag | Effect |
| --- | --- |
| `-e`, `--endpoint NAME` | Use endpoint NAME. A bare endpoint name as the first word does the same. |
| `-p`, `--print` | Run one prompt and exit. A bare prompt after the options does the same. |
| `-c`, `--continue` | Resume the last session in this directory. |
| `--fork` | Copy the resumed session (`-c`, `--session`) and continue the copy. Not with `--goal` or `--steps`. |
| `--trust` | Read workspace `SYSTEM.md`, `AGENTS.md`, and `CLAUDE.md`. On by default. |
| `--no-trust` | Ignore those workspace files. |
| `--net` | Allow network inside the sandbox. On by default. |
| `--no-net` | Deny network inside the sandbox. |
| `--no-verify` | Skip the `.summonr/check.toml` gate. |
| `--no-splash` | Skip the startup banner. |
| `--inline` | Keep the transcript in terminal scrollback, with the composer pinned at the bottom. |
| `--here` | Work in this checkout. This is the default; it cannot be combined with `--worktree`. |
| `--model NAME` | Use model NAME on the endpoint instead of its default. |
| `--effort LEVEL` | Set the reasoning effort: `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, or `max`. See Reasoning effort. |
| `-w`, `--workflow NAME` | Seat each step from `[workflows.NAME]`. |
| `--json` | Print JSON: one event per line with `-p`, `--goal`, and `--steps`, and JSON output from `diff`, `worktrees`, `mcp`, and `classify`. |
| `--session ID` | Resume a session by id, short id, prefix, or name. |
| `--name TEXT` | Name this session. |
| `--goal TEXT` | Work toward TEXT across turns until the check passes and the judge says `done`. |
| `--steps PLAN.md` | Run each item under `## Steps` in PLAN.md and gate it on its check. |
| `--max-tokens N` | Session token budget. A breach stops the run. |
| `--max-secs N` | Session wall-clock budget in seconds. A breach stops the run. |
| `--sandbox` | Contain shell commands. Writes stay under the workspace and extra writable roots. |
| `--no-sandbox` | Run uncontained, even with `on = true` under `[sandbox]`. |
| `--add-dir DIR` | Add an extra writable root. Repeatable. |
| `--strict` | Allow only the commands listed in `policy.toml`. |
| `--worktree` | Isolate this session in a git worktree. |
| `--gui` | Open the desktop window. Needs a build with `--features gui`. |
| `-h`, `--help` | Show the help. |
| `-V`, `--version` | Show the version. |

The last of a pair (`--trust`/`--no-trust`, `--net`/`--no-net`) wins. An option that takes a value rejects a value that starts with `-`: `--name -x` is an error, not the name `-x`.

`summonr -p` with no prompt reads the prompt from stdin until EOF. So does `summonr` with no prompt when stdin or stdout is not a terminal:

```sh
git diff | summonr kimi -p
```

### Subcommands

| Command | Job |
| --- | --- |
| `resume [ID\|NAME]` | Resume a session; with no argument, the last one in this directory. |
| `sessions [--grep TEXT]` | List sessions, or search their text. |
| `export [ID\|NAME] [--digest]` | Print a session as Markdown; `--digest` prints the condensed form that `classify --review` reads. |
| `profile [ID\|NAME]` | Time and tokens of one session. `--all` totals every session, and `--last N` only the newest N. |
| `diff [PATH]` | Changes against the base. Takes `--stat`, `--json`, and `--turn N\|last`. |
| `worktrees [prune] [--json]` | List summonr worktrees, or remove the ones that are gone. |
| `endpoints` | List endpoints, their models, and each key state. |
| `login [NAME]` | List keys and plans, or sign in to NAME. Takes `--key`, `--url URL`, `--model NAME`, and `--default`. |
| `logout NAME [--remove]` | Forget the stored key or token; `--remove` deletes an endpoint you added. |
| `workflows` | List saved workflows. |
| `modules` | List the built-in modules. |
| `mcp list\|get NAME [--json]` | List MCP servers with their state and tool counts, or show one server and its tools. |
| `mcp add NAME -- CMD [ARGS]` | Add a stdio server; `--env K=V` sets its environment. `mcp add NAME --url URL` adds an HTTP server; `--header K=V` adds a request header. Both flags repeat. |
| `mcp remove NAME` | Remove an MCP server from `~/.summonr/config.toml`. |
| `install PATH [--list]` | Install a directory of extras; `--list` shows installed plugins. |
| `uninstall NAME` | Remove an installed plugin. |
| `doctor [--logs]` | Report crashed runs; `--logs` lists the run logs. |
| `acp` | Serve the Agent Client Protocol on stdio. |
| `mail` | Peer mail between sessions: `agents`, `send`, `inbox`, `board`. See Mail. |
| `tool NAME [JSON]` | Run one tool with JSON arguments (default `{}`) in this directory and print the result. Exits 1 when the tool fails. `--sandbox` contains it. |
| `classify --set NAME [STATE]` | Answer a named question set about STATE, read from arguments or stdin. |
| `classify --questions @FILE [STATE]` | Answer the questions in FILE. |
| `classify --noul QUESTION [STATE]` | Answer a yes/no question. Repeatable. |
| `classify --patch` | Rank the files of the working patch by risk. |
| `classify --review --session ID` | Review a session. |
| `classify --set NAME --eval FILE` | Score a question set against labeled cases in FILE. `--json` prints answers and probabilities, and `--gate N` sets the confidence floor for every `classify` form. |
| `cron add NAME SCHEDULE TASK` | Add a job. SCHEDULE is `--cron EXPR`, `--every 30m`, `--at TIME`, or `--manual`; TASK is `-p PROMPT` or `--goal GOAL`. Takes `--dir`, `-e`/`--endpoint`, `--model`, `--tz`, `--max-secs`, `--max-tokens`, `--worktree`, `--chat`, `--notify never\|failure\|always`, and `--mail SESSION`. See Cron. |
| `cron edit NAME [FLAGS]` | Change a job with the same flags as `add`. |
| `cron list [--json]` | List jobs. |
| `cron show NAME` | Show one job, including the endpoint its runs use. |
| `cron run NAME [--raw] [--notify never\|failure\|always]` | Run a job now; `--notify` overrides the job's setting for this run. |
| `cron log NAME [--last N] [--raw]` | Past runs, then the last run's answer, verdict, and log path. |
| `cron pause NAME`, `cron resume NAME` | Stop or restart a job's schedule. |
| `cron remove NAME` | Delete a job and its token. |
| `cron token NAME` | Print a new webhook token for a job; the old one stops working. |
| `cron install`, `cron uninstall`, `cron status` | Register or remove the OS tick that starts due jobs, or say whether it is installed and when the next job fires. Without the tick, nothing fires, and `next` says when a job would fire. |
| `gateway setup telegram` | Store the Telegram bot token. |
| `gateway telegram` | Run the Telegram bridge. Takes `--endpoint E`, `--model M`, and `--dir D`. See Gateway. |
| `gateway pair CODE` | Allow the Telegram account that received CODE. |
| `gateway unpair USER_ID` | Remove a paired account, its chat, and its queued messages. |
| `gateway status` | List paired accounts. |
| `gateway webhook [--port N]` | Listen on `127.0.0.1` (port `gateway_port`) for `POST /run/NAME`. |

### Config keys

Top-level keys in `~/.summonr/config.toml`:

| Key | Values | Effect |
| --- | --- | --- |
| `splash` | `true` (default), `false` | Show the startup banner. |
| `models_catalog` | `true` (default), `false` | Fetch the models.dev catalog for `summonr endpoints` and `/model`; `false` keeps every request on your own endpoints. |
| `animations` | `true` (default), `false` | Animate the interface. |
| `fps` | 1 to 240, default 25 | Interface frame rate. |
| `diff_layout` | `split` (default), `unified` | Diff pane layout. |

Keys on an `[endpoint.NAME]` row:

| Key | Effect |
| --- | --- |
| `context_window` | Context size in tokens. Without it, summonr asks the provider. |
| `idle_timeout_secs` | Seconds a response stream may stay silent before the request fails. At least 1; default 240. |
| `headers` | A table of extra HTTP headers sent on every request. |
| `fallback` | A list of endpoint names. When a request fails with an error that a retry cannot fix, the session moves to the next name. |
| `compact_tokens` | Compact the context at this token count. With `context_window` also set, the smaller value applies. |
| `keep_reasoning` | `true` sends earlier reasoning back to the model on later requests. |
| `cache_ttl` | Prompt-cache lifetime on the Anthropic wire: `"1h"` or `"5m"`; unset uses `[limits] cache_ttl`. A cache write costs 2x the input price at `1h` and 1.25x at `5m`; a read costs 0.1x either way. `grok` also sends `x-grok-conv-id` with the prompt hash so xAI routes each request to a warm cache. |
| `auth.token_url` | Token endpoint of a login. Required with `auth.client_id`. |
| `auth.label` | Name of the subscription shown when an endpoint offers both a sign-in and a key. |
| `auth.required_scope` | A scope the token reply must grant, or the sign-in fails. |
| `auth.device_url` | Device authorization endpoint for `grant = "device_code"`. |

`[limits]` in `~/.summonr/config.toml` overrides the built-in values in [`src/limits.toml`](../../src/limits.toml): every timeout, retry, size, retention, and UI timing summonr uses. A key you leave out keeps its built-in value.

```toml
[limits]
retry_after_max_secs = 30
cache_ttl = "5m"
```

| Key | Built in | Meaning |
| --- | --- | --- |
| `cache_ttl` | `"1h"` | Prompt-cache lifetime for an endpoint that sets none: `"5m"` or `"1h"`. |
| `retry_limit` | `3` | Retries for a 429, a 5xx, or a connection error. |
| `retry_base_ms` | `1000` | First retry wait; each retry doubles it. |
| `retry_max_ms` | `8000` | Longest retry wait. |
| `retry_after_max_secs` | `60` | Longest `Retry-After` summonr waits for; a longer one falls back to the next endpoint. |
| `stream_idle_secs` | `240` | Idle stream timeout for an endpoint with no `idle_timeout_secs`. |
| `stream_total_secs` | `86400` | Total time limit for one request. |
| `connect_secs` | `20` | Connect timeout. |
| `compact_percent` | `80` | Share of the context window that starts compaction. |
| `elide_percent` | `60` | Share of the context window that compaction aims for by clearing old tool output; it summarizes only when clearing cannot get there. |
| `compact_chars` | `400000` | Prompt size in characters that starts compaction when the window is unknown. |
| `summary_retry_rounds` | `8` | Rounds before a failed compaction summary retries. |
| `job_wait_secs` | `120` | Wait on a job or hook before it is reported still running. |
| `command_wait_secs` | `120` | Time before a `run_command` continues as a background job. |
| `command_kill_secs` | `3600` | Hard stop for a foreground command with no `timeout_secs`. |
| `grant_wait_secs` | `600` | Wait for a permission answer. |
| `job_poll_start_secs` | `1` | First job poll interval. |
| `job_poll_cap_secs` | `2` | Longest job poll interval. |
| `mail_stale_secs` | `120` | A mail peer with no heartbeat for this long is stale. |
| `mail_sweep_secs` | `86400` | Mail entries older than this are removed. |
| `verify_timeout_secs` | `600` | Timeout for a check command. |
| `probe_secs` | `5` | Endpoint probe timeout. |
| `auth_secs` | `30` | Sign-in request timeout. |
| `model_probe_secs` | `8` | Model list request timeout. |
| `web_search_secs` | `15` | Web search timeout. |
| `search_secs` | `20` | Time one `search` call walks and scans before it returns what it found. |
| `search_index_after_ms` | `1000` | A name search outside the workspace that walks at least this long builds a file index in the background. |
| `search_index_secs` | `300` | Age at which a file index is rebuilt. |
| `models_catalog_secs` | `3600` | Age at which the models.dev catalog is fetched again. |
| `web_connect_secs` | `8` | Web connect timeout. |
| `web_fetch_secs` | `20` | Web fetch timeout. |
| `web_body_bytes` | `1048576` | Largest web response body. |
| `web_redirect_max` | `5` | Most redirects a web request follows. |
| `keychain_ms` | `5000` | OS keychain call timeout. |
| `keychain_poll_ms` | `20` | Keychain poll interval. |
| `mcp_open_secs` | `60` | MCP server start timeout. |
| `mcp_call_secs` | `300` | MCP tool call timeout when a server sets none. |
| `mcp_idle_secs` | `120` | MCP idle stop when a server sets none. |
| `lsp_idle_secs` | `300` | Language-server idle stop. |
| `servers_max` | `8` | MCP and language servers live at once; past it the least recently used idle one stops. |
| `receipt_items_max` | `5` | Failures and changed files listed in an expanded turn receipt. |
| `closest_percent` | `34` | A name is offered as the closest match only when it differs by at most this percent of the name length. |
| `typo_min_chars` | `3` | An endpoint name with at least this many characters also gets a hint for one changed, added, or removed character, or two swapped adjacent characters. |
| `diff_split_min_cols` | `100` | Narrowest diff pane that shows hunks side by side; a narrower pane shows the unified diff. |
| `mcp_progress_stretch` | `4` | Progress can extend an MCP call up to this multiple of its timeout. |
| `mcp_cancel_write_secs` | `1` | Time to write an MCP cancel. |
| `mcp_close_grace_ms` | `500` | After closing a stdio MCP server's input, how long summonr waits for it to exit before it stops the process. |
| `image_side_px` | `2000` | Longest side of an image sent to the model; a larger image is scaled down. |
| `image_bytes` | `5000000` | Largest image sent to the model; a larger one is re-encoded to fit. |
| `mcp_log_max_bytes` | `1048576` | MCP log size before rotation. |
| `mcp_frame_max_bytes` | `4194304` | Largest MCP reply. |
| `mcp_stderr_tail_bytes` | `2048` | MCP stderr kept for diagnostics. |
| `mcp_stderr_settle_secs` | `1` | Most time spent draining MCP stderr after stdout closes. |
| `mcp_defer_bytes` | `8192` | Tool schemas above this size go behind one `mcp` tool. |
| `mcp_found_max` | `8` | Most deferred tools returned per search. |
| `lsp_start_secs` | `30` | Language server start timeout. |
| `lsp_request_secs` | `30` | Language server request timeout. |
| `lsp_check_secs` | `5` | Diagnostics wait after an edit. |
| `log_retain_days` | `14` | Log retention. |
| `session_retain_days` | `30` | Session retention when `[sessions] retain_days` is unset. |
| `job_retain_days` | `7` | Finished job retention. |
| `speed_window_secs` | `3` | Window for the tokens-per-second figure. |
| `reader_window_ms` | `400` | Stream batching window. |
| `frame_window_secs` | `1` | Frame budget window. |
| `animation_ms` | `40` | Animation frame period. |
| `spinner_ms` | `100` | Spinner frame period. |
| `splash_reveal_ms` | `760` | Splash reveal time. |
| `status_refresh_secs` | `1` | Status line refresh. |
| `beat_secs` | `2` | Heartbeat interval. |
| `notice_secs` | `2` | Time a notice stays on screen. |
| `worker_stop_secs` | `2` | Desktop worker stop wait. |
| `repeat_window_secs` | `2` | Window for a double key press. |
| `step_attempts` | `3` | Attempts per plan step. |
| `repair_limit` | `6` | Argument repairs and overflow retries per turn. |
| `repeat_nudge` | `5` | Times the same tool call, or the same cycle of up to three calls, may return the same result in a row before summonr adds a note to the result asking the model to change approach. The call still runs, and a new message starts the count again. `0` turns the note off. |
| `edit_hunk_rows` | `80` | Rows shown per edit hunk. |
| `display_patch_lines` | `1000` | Most patch lines shown. |
| `token_expiry_skew_secs` | `60` | Refresh a sign-in token this early. |
| `read_line_limit` | `1000` | Largest `read` line limit. |
| `paste_fold_lines` | `8` | A paste of this many lines folds. |
| `paste_fold_chars` | `1000` | A paste of this many characters folds. |
| `cron_tick_secs` | `60` | Interval at which the OS runs `summonr cron tick` after `summonr cron install`. |
| `cron_budget_secs` | `1800` | Time budget of a cron run that sets no `--max-secs`. |
| `cron_runs_kept` | `50` | Runs kept per cron job; older runs and their saved webhook bodies are deleted. |
| `cron_catchup_secs` | `604800` | A missed cron time older than this is logged as expired instead of run. |
| `cron_fail_pause` | `3` | Failed cron runs in a row before the job pauses itself. |
| `memory_bytes` | `25000` | Bytes of each memory file loaded into a session. |
| `gateway_poll_secs` | `50` | Long-poll wait of each Telegram update request. |
| `gateway_pair_secs` | `3600` | Lifetime of a Telegram pairing code. |
| `gateway_port` | `8765` | Port `summonr gateway webhook` listens on without `--port`. |
| `gateway_body_bytes` | `65536` | Largest webhook body; a larger one answers `413`. |
| `gateway_fire_secs` | `60` | A second webhook fire of one job within this time answers `429`. Scheduled runs and `cron run` do not count. |
| `gateway_head_secs` | `2` | Time a webhook client has to send its headers. |
| `gateway_read_secs` | `10` | Time a webhook request has to finish; a slower one answers `408`. |
| `gateway_stop_secs` | `5` | After `/stop` in a chat, wait this long before the remaining commands are killed. |

### Environment variables

| Variable | Effect |
| --- | --- |
| `SUMMONR_HOME` | summonr home directory. Default `~/.summonr`. |
| `SUMMONR_ENDPOINT` | Endpoint to start on when none is named on the command line. |
| `SUMMONR_MODEL` | Model to start on when `--model` is not given. |
| `SUMMONR_SECRETS` | Credential store: `file` (the default) or `keychain`. Overrides `secrets` in `config.toml`. |
| `SUMMONR_MAIL` | `off` hides this session from peer mail and turns off the mail tools. |
| `SUMMONR_PLUGIN_ROOT`, `SUMMONR_PLUGIN_DATA` | Set by summonr for a plugin's hooks: the plugin directory and its data directory. |
| `SUMMONR_BASE_URL` | Use this OpenAI-compatible base URL as the endpoint when no endpoint is named. |
| `SUMMONR_API_KEY` | API key for `SUMMONR_BASE_URL`. |
| `SUMMONR_MAX_TOKENS` | `max_tokens` sent with `SUMMONR_BASE_URL`. |
| `SUMMONR_KEEP_REASONING` | When set, `SUMMONR_BASE_URL` keeps reasoning as `keep_reasoning = true` does. |
| `SUMMONR_SYSTEM_PROMPT` | Replace the whole system prompt. `SYSTEM.md`, `AGENTS.md`, `CLAUDE.md`, and skills are not added. |
| `SUMMONR_MODELS_URL` | URL of the model catalog. |
| `SUMMONR_CRON` | Set by summonr to the job name in a cron run. `summonr cron add`, `edit`, and `remove` refuse while it is set. |
| `SUMMONR_CAPTURE_DIR` | Write raw provider request and stream frames as JSON lines to this directory. |
| `SUMMONR_VERSION` | Installer: release tag to install. Default `latest`. |
| `SUMMONR_INSTALL_DIR` | Installer: install directory. |
| `SUMMONR_LIBC` | `install.sh` on Linux: `musl` (default) or `gnu`. |

### Reasoning effort

The levels are `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, and `max`. Each model accepts some of them. summonr sends the nearest level the model accepts, and the lower one when two are equally near. `max` on a model that stops at `high` sends `high`. `medium` on a model that accepts only `low` and `high` sends `low`.

A model that takes no effort setting gets no effort field, so a level you pick cannot fail the request. With no effort set and none remembered, summonr uses `high` for a model the catalog marks as reasoning. For any other model it sends no effort field, and the model uses its own default.

`none` turns thinking off where the model can turn it off. If the model cannot, summonr sends the lowest level it accepts.

Anthropic Messages (`anthropic`, or any row with `wire = "anthropic"`), by model id:

| Model | Levels | `none` |
| --- | --- | --- |
| `claude-opus-5-5`, `claude-fable-5`, `claude-mythos-5` | `low`, `medium`, `high`, `xhigh`, `max` | sends `low` |
| `claude-mythos-preview` | `low`, `medium`, `high`, `max` | sends `low` |
| `claude-opus-5`, `claude-sonnet-5` | `low`, `medium`, `high`, `xhigh`, `max` | `thinking` type `disabled` |
| `claude-sonnet-5-5` | `low`, `medium`, `high`, `xhigh`, `max` | `thinking` type `between_tools` |
| `claude-opus-4-8`, `claude-opus-4-7` | `low`, `medium`, `high`, `xhigh`, `max` | no `thinking` field |
| `claude-opus-4-6`, `claude-sonnet-4-6` | `low`, `medium`, `high`, `max` | no `thinking` field |
| `claude-opus-4-5` | `low`, `medium`, `high` | no `thinking` field |
| Earlier Claude models | `low`, `medium`, `high`, sent as a thinking budget | no `thinking` field |

OpenAI (`openai`, on the chat or the Responses wire), by model id:

| Model | Levels | `none` |
| --- | --- | --- |
| `gpt-6`, `gpt-5.6` | `low`, `medium`, `high`, `xhigh`, `max` | sends `none` |
| `gpt-6-astra` | `low`, `medium`, `high`, `xhigh`, `max` | sends `low` |
| `gpt-5.5`, `gpt-5.4`, `gpt-5.2` | `low`, `medium`, `high`, `xhigh` | sends `none` |
| `gpt-5.1` | `low`, `medium`, `high` | sends `none` |
| `gpt-5` | `minimal`, `low`, `medium`, `high` | sends `minimal` |
| `gpt-5-pro` | `high` | sends `high` |
| Other `gpt-5` models that contain `-pro` | `medium`, `high`, `xhigh` | sends `medium` |
| `gpt-5.3-codex`, `gpt-5.2-codex` | `low`, `medium`, `high`, `xhigh` | sends `low` |
| Other `codex` models | `low`, `medium`, `high` | sends `low` |
| `gpt-5-chat`, `gpt-4`, `gpt-3` | no effort setting | sends nothing |
| Any other model, such as `o3` | `low`, `medium`, `high` | sends `low` |

Gemini (`gemini`), by model id. Levels for `gemini-2.5` models set the thinking budget:

| Model | Levels | `none` |
| --- | --- | --- |
| `gemini-2.5-pro` | `low`, `medium`, `high` | sends the `low` budget |
| Other `gemini-2.5` models | `low`, `medium`, `high` | sets the budget to 0 |
| `gemini-3-pro` | `low`, `high` | sends `low` |
| `gemini-3` models that contain `pro`, `3.7`, or `3.8` | `low`, `medium`, `high` | sends `low` |
| Other `gemini-3` models | `minimal`, `low`, `medium`, `high` | sends `minimal` |

Other built-in endpoints:

| Endpoint | Levels | `none` |
| --- | --- | --- |
| `zai`, `zai-cn`, `glm`, `glm-intl` | `glm-5.3`: `low`, `high`, `max`. `glm-5.2`: `minimal` to `max`. Earlier models: no effort setting. | `glm-5.3`: sends `low`. `glm-5.2`: sends `none`. Earlier models: `thinking` type `disabled`. |
| `deepseek` | `minimal` to `max` | `thinking` type `disabled` |
| `kimi` | `k3`: `low`, `high`, `max`. `kimi-k2` models: no effort setting. | `k3`: sends `low`. `kimi-k2.6`: `thinking` type `disabled`. Other `kimi-k2` models: sends nothing. |
| `grok` | `grok-4.7` and `grok-4.6`: `low` to `xhigh`. Others: `low`, `medium`, `high`. | sends the lowest level |
| `muse` | `minimal` to `xhigh` | sends `minimal` |
| `mistral` | `high` | sends `none` |
| `cerebras`, `deepinfra`, `ollama` | `low`, `medium`, `high` | sends `none` |
| `openrouter` | `minimal` to `xhigh`, sent as `reasoning` with an `effort` key | sends `none` |
| `qwen-plan`, `qwen-plan-cn`, `dashscope`, `dashscope-cn` | `low`, `medium`, `high` | sends nothing |
| `together`, `groq`, `copilot`, `zen`, `go`, `opencode`, `fireworks` | `low`, `medium`, `high` | sends nothing |
| `minimax`, `minimax-cn` | no effort setting | `thinking` type `disabled` |
| `minimax-plan`, `minimax-cn-plan` | `low`, `medium`, `high` | sends nothing |
| `tencent-plan`, `volcengine-plan`, `siliconflow`, `siliconflow-cn`, `nvidia`, `huggingface`, `cohere`, `perplexity`, `ollama-cloud`, `lmstudio` | `tencent-plan` and `volcengine-plan`: no effort setting. Others: `low`, `medium`, `high` | sends nothing |

summonr matches the model id against these model rows on every endpoint except `copilot`, `openrouter`, `ollama`, `lmstudio`, `tencent-plan`, and `volcengine-plan`, which pass any model through and use the endpoint's own levels. A model that matches no row uses the endpoint's levels. An endpoint with no built-in host uses `low`, `medium`, and `high`, and sends nothing for `none`.

Set the levels or the `none` style on an endpoint to replace the built-in behavior. A `[endpoint.NAME.models.ID]` row replaces it for one model.

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8080/v1"
model = "qwen3"
reasoning_levels = ["low", "high", "max"]
thinking_off = "disabled"

[endpoint.local.models.qwen3-small]
reasoning_levels = []
```

`reasoning_levels` lists the levels the endpoint accepts, from `minimal` to `max`. An empty list sends no effort. `thinking_off` sets what `none` sends:

| `thinking_off` | `none` sends |
| --- | --- |
| `omit` | nothing |
| `none` | the level `none` |
| `disabled` | `thinking` with type `disabled` |
| `between_tools` | `thinking` with type `between_tools` |
| `chat_template_kwargs` | `chat_template_kwargs` with `enable_thinking` set to `false` |
| `unsupported` | the lowest level in `reasoning_levels` |

The `body` table of an endpoint is merged last, so it can still set any field by hand.

### Modes

| Mode | Enter | Leave | Job |
| --- | --- | --- | --- |
| Auto | default, `/auto` | `/goal`, `/steps` | Follow the request this turn. |
| Work | `/steps FILE`, `--steps` | steps end (auto), `/auto` | Run a plan's `## Steps` one at a time, each with its check. |
| Goal | `/goal TEXT`, `--goal` | judge `done` / stop, `/auto`; the session returns to the mode it had before | Loop on an objective until checks pass and the judge says `done`. |

`/goal` with no check configured uses a check detected from the project: `cargo test` for `Cargo.toml`, `go test ./...` for `go.mod`, and `npm test` when `package.json` declares a `test` script. A Python project gets no detected check, and the judge decides. `/goal` prints the detected check and says how to set another in `.summonr/check.toml`.

### Confirmation

Everything runs without a prompt except irreversible destruction: wiping the system, home, or project root, `mkfs`, `dd` to a device, a fork bomb, `chmod -R 777 /`, `terraform destroy`, cluster deletes, and dropped database tables. Those ask first with `[ allow ]` / `[ deny ]` (or `y` / `n`). `-p` has no one to ask, so they do not run there. Each turn's checkpoint makes the rest, such as `git reset --hard`, recoverable with `/undo`.

`sudo` asks for your password in a dialog. The password goes to `sudo` through `SUDO_ASKPASS`; the model never sees it, and command output is scrubbed of it.

### Workflows

A workflow runs each role on its own model. Roles are `scout`, `implement`, `review`, `judge`, and `summary`; a role the workflow does not name runs on the endpoint and model you started with.

```toml
# ~/.summonr/config.toml
workflow = "cheap-scout"      # optional default

[workflows.cheap-scout]
scout = "grok"
review = "zai"
judge = "grok"
```

`summonr -w cheap-scout`, `/workflow cheap-scout` in the interface, or the `workflow` key picks one; `summonr workflows` lists them. In the interface every card action has a verb too:

```
/workflow                    the card
/workflow NAME | off         apply a saved workflow, or none
/workflow list | show        saved workflows · the live seats
/workflow set ROLE ENDPOINT [MODEL]
/workflow clear ROLE
/workflow save NAME          write the live seats as [workflows.NAME]
/workflow default NAME       the workflow new sessions start with
```

## Interface

`summonr --gui` opens the session in a desktop window (a build with `--features gui`). The window is built on Sigil: a left rail that switches the Files, Changes, Worktrees, Jobs, Peers, and Ledger panes and lists this workspace's sessions; a main column with the transcript, and a composer card; a right rail that shows the open pane with real widgets: clickable file rows, +/− chips that open a file's hunks, an Edit/Save file view. The title bar's changes badge is live. The same `App`, keys, and layouts drive the terminal and the window.

The screen is a set of panes on one session. `ctrl+x e` (or `/pane files`) opens the repository tree beside the chat; `ctrl+t` the plan ledger; `ctrl+x b` hides or restores the pane. In the tree: `ctrl+→` focuses it, `↑↓` move, `←→` collapse or expand, `PgUp/PgDn/Home/End` jump, Enter opens a directory or attaches `@path` to your prompt, and a right-click (or the mouse) opens a menu: attach, open in `$EDITOR`, insert path. `ctrl+←` or `esc` returns to the composer. With the composer focused, an empty draft, no open overlay, and no running turn, `esc` closes the open side pane. `/pane off` closes the pane. `/pane focus`, `run`, and `review` are presets, `/pane "chat | files 30%"` sets the split, and ctrl+shift+←/→ resizes it. The layout persists in `~/.summonr/config.toml` as `layout`. `ctrl+x o` opens the settings modal, where every hotkey is editable; the choice is stored under `[hotkeys]` in the same file. `ctrl+x n` folds every thinking block, then every tool output (`tools = "hidden"`), then shows them all again; a click still opens one block.

`ctrl+x d` (or `/pane diff`, `/diff`) shows the changes against the base: the worktree base when you passed `--worktree`, `HEAD` otherwise. Each row is `+added −removed path`; Enter or `→` opens that file's hunks, one card per `@@` block, with a `pending`, `accepted`, or `rejected` badge. Inside a file `↑↓` picks a hunk, `a` toggles `accepted` (Enter does not), `r` reverts the hunk (`git apply --reverse` of just that block), `n` opens a note field under the hunk header (Enter saves, `esc` cancels; a saved note shows there) that rides to the model with your next prompt, and `←` or `esc` returns to the list. While the pane has focus, typed letters stay in the pane, and `esc` from the list returns to the composer. The right-click menu offers the same actions. Verdicts and notes persist in the repository's git directory, `.git/summonr/review.json`, or in `.summonr/review.json` outside a git repository. `/diff PATH` jumps to one file. After a turn that changed the tree, the transcript gets one `changes: N files · +a −r · vs BASE · /pane diff` line. The same data is `summonr diff [PATH]` (the patch), `summonr diff --stat`, and `summonr diff --json`. `summonr diff` never lists generated caches (`__pycache__/`, `*.pyc`, `.pytest_cache/`, `.mypy_cache/`, `.ruff_cache/`) or summonr state under `.summonr/` other than `check.toml` and `policy.toml`, even without a `.gitignore`.

`→` on a file in the tree (or `/view PATH`, or the menu's view file) reads it in the view pane with line numbers; lines the diff added are green with a `+`. `↑↓`, `PgUp/PgDn`, `Home/End` scroll, `o` opens it in `$EDITOR`, `esc` returns to the tree. `e` edits the file in place: the keyboard goes to the file (type, Enter, arrows, Home/End), the caret is the inverted cell, `ctrl+s` saves, and `esc` leaves. With unsaved text a second `esc` discards it. The tree, the diff, and the view follow the file system: a change on disk relists, re-diffs, or reloads the pane within about half a second.

In the diff pane `t` keeps only the files the last turn edited (the header says `· last turn`), `c` copies the patch on show to the clipboard (OSC 52 in the terminal; the system clipboard in the window), and a pane `diff_split_min_cols` columns or wider (100 by default) shows a file's hunks side by side, old on the left; a narrower pane shows the unified diff. `summonr diff --turn N` shows what turn N changed: from the checkpoint taken at its start to the next turn's checkpoint, or to the workspace for the newest turn. `--turn last` is the newest turn that changed files, in the newest session for this workspace. Without `--stat` or `--json`, it writes the same summary line as `--stat` to stderr before the patch. A resumed session keeps numbering its turns. The summary reads `turn N of session <short id>`; the JSON has `base: "turn N"` and `session` with the session id. A diff of the whole workspace still reads `vs HEAD`. A file that summonr's own output is redirected to stays out of every later checkpoint in that workspace, in every session, so `--turn` never lists it. A change to `.summonr/check.toml` or `.summonr/policy.toml` is never a touched path.

While a turn runs, a row above the composer shows a spinner, what the model is doing (`thinking`, or the tool it is running), the round, the seconds so far, the output rate, and the queue count. A message sent during the turn goes into the running turn, and the model reads it after its current tool calls. A message waits in the queue for the next turn instead while an approval or a question is open, while earlier messages or commands still wait, or when it starts with `!` or `/`; `enter` on an empty composer sends the waiting plain messages into the running turn.

The output rate is tokens per second. While text streams it is an estimate: characters received in the last three seconds, divided by the characters per token measured on the previous round (4 until then), over that span. When the provider reports the round's usage, the rate becomes exact: output tokens over the time between the first and last streamed chunk, or over the whole round when nothing streamed, as in a round that only called tools.

When a turn ends while the terminal is not focused, summonr rings the bell and sends a desktop notification (OSC 9) naming the workspace. `notify` in `config.toml`, or the Notify row in `/settings`, sets `off`, `turn` (the default), or `all`, which also notifies when an approval is waiting.

The footer shows the workspace and branch on the left and, on the right, the session's spend when the model has a price or the provider reports a cost, then the context and cache meters.

When a command asks for a password through `SSH_ASKPASS` or `SUDO_ASKPASS`, such as an ssh key passphrase, a **Credential required** dialog opens. Type the password and press `enter` to send it, or press `esc` to cancel. Closing the dialog also cancels the request. The password goes to the waiting command over a private socket, never to the model, and later command output shows it as `[secret]`.

### Keys

| Key | Idle composer | During a turn |
| --- | --- | --- |
| `enter` | send | send into the running turn after the current tool calls, or queue it for the next turn as described above; on an empty composer, send the waiting plain messages into the running turn |
| `tab` | complete a `/command`, its argument, `@path`, a `/server:prompt`, or an `@server:uri`; in `/model`, cycle the filter: all, ready, ★, plan, free, think, vision, ≥1M; otherwise cycle reasoning effort | effort takes effect when the turn ends |
| `↑` | recall earlier prompts; with input queued, pull it back into the editor | |
| `esc` | dismiss; twice within two seconds opens the session tree | cancel the turn |
| `ctrl+c` | clear the draft; on an empty draft, twice within two seconds quits | cancel the turn |
| `ctrl+d` | quit when the draft is empty; otherwise delete the character after the caret | |
| `pgup` `pgdn` | scroll the transcript a page | same |
| `home` `end` | on an empty draft, jump to the top or the latest reply | same |
| `alt+d` | in the sessions picker, twice deletes the selected session; in `/login`, signs out; in `/model`, sets the default | |
| `alt+k` | in `/login`, pastes a key on a row that also offers a sign-in | |
| `alt+p` | in `/model`, adds or removes the selected model from favorites | |
| `alt+o` | in `/model`, shows or hides models that are older than six months or have a newer model in the same family | |
| `F5` | retry a failed turn; in `/model`, refresh the providers | |
| `←` `→` | in `/model`, `/login`, and `/usage`, switch between the Models, Providers, and Usage tabs; `ctrl+←` `ctrl+→` also work, and `↑` `↓` change the range in Usage | |
| `shift+enter` `alt+enter` | newline; macOS Terminal sends `alt+enter` for Shift+Return | same |
| `ctrl+j` | newline (default; editable in options) | |
| `ctrl+g` | edit the draft in `$EDITOR` (default) | |
| `shift+tab` | toggle approval for risky commands | takes effect when the turn ends; configured policy still applies |
| `alt+←` `alt+→` `alt+b` `alt+f` | word left or right | |
| `ctrl+w` `alt+backspace` | delete the word before the caret | |
| `alt+d` | delete the word after the caret | |
| `ctrl+u` `ctrl+k` | delete to the line start or end | |
| `ctrl+a` `ctrl+e` | start or end of the line | |
| `ctrl+b` `ctrl+f` | one character left or right | |
| `ctrl+p` `ctrl+n` | older or newer prompt, same as ↑ and ↓ | |
| `ctrl+l` | redraw the screen | |
| `ctrl+x` | then a letter runs that chord | |

### Commands

`/help` lists these by job. Old names such as `/rollback`, `/tree`, and `/net` still work for one release and print the new name once.

| Command | Job |
| --- | --- |
| `/resume [ID\|NAME]` | open another session |
| `/new` | start another session beside this one |
| `/name [TEXT]` | name this session |
| `/clear` | start a fresh context; files stay |
| `/compact` | summarize earlier turns |
| `/retry` | continue a failed or cancelled turn (F5) |
| `/undo [files [N]]` | undo the last turn; files N restores files |
| `/fork [TEXT]` | fork this session; TEXT picks a turn |
| `/export [PATH]` | write this session as markdown |
| `/copy [all]` | copy the last reply; all copies everything |
| `/search TEXT` | find text in this conversation |
| `/history [TEXT]` | reuse an earlier prompt |
| `/status` | session id, log path, and model |
| `/mcp` | MCP servers: state, tools, schema size, last error |
| `/usage [session]` | token spend; session profiles this session |
| `/agents` | list agent sessions on this machine |
| `/memory` | show the memory files and their size |
| `/exit` | quit summonr |
| `/model [NAME] [LEVEL]` | choose the model; LEVEL sets the effort |
| `/effort [LEVEL]` | set the reasoning effort; no LEVEL opens a slider |
| `/workflow [NAME]` | choose models for scout, work, and review |
| `/login [NAME]` | sign in to a provider or store its key. Rows sit under `ready`, `needs attention`, and `not set up`; at 96 columns or more a detail pane shows the row's key, sign-in, models, and actions |
| `/auto` | return to auto mode |
| `/goal TEXT` | work toward an objective across turns |
| `/steps PLAN.md\|clear` | run a plan's `## Steps` one at a time |
| `/diff [PATH]` | changes against the base, or one PATH |
| `/view PATH` | read a file in a pane |
| `/pane [NAME]` | files, ledger, diff, worktrees, jobs, peers, or a layout |
| `/settings` | theme, tool output, reasoning, modules, incoming mail, `NAME tools` per MCP server, keys |
| `/permissions [net\|write]` | sandbox network and write paths |
| `/help` | list commands and keys |

Typing `/` opens the command list above the composer. Each row shows the command, its argument, and one line of help. After `/model `, or `/login `, the list shows the values that command takes: the current endpoint's cached models or the endpoint names. `↑` and `↓` move, `tab` completes, `enter` runs the selected row (prompt rows too), and `esc` hides the list. An unknown command says so and names the closest command; it never runs another command. One `esc` closes `/login` and `/model`. With no usable endpoint, the composer header reads `no provider · /login`. The `ctx` meter hides when the model's context window is unknown.

A draft that starts with `!` runs in the command sandbox before any model round: `!cmd` shows the output as a tool card and then sends `$ cmd` plus the output as the prompt; `!!cmd` shows the output only. Escape cancels either command and stops its process group; a cancelled command does not start a model response.

On macOS, `ctrl+v` or `alt+v` in the terminal attaches an image from the system clipboard; the desktop window also accepts `cmd+v`. An image pasted from the clipboard, or attached with `@shot.png`, shows in the transcript, and so does any image the model shows you. `read` on an image path, or a markdown image link such as `![plot](out/plot.png)` in a reply, shows that file. In Ghostty, Kitty, WezTerm, and Konsole the picture renders through the Kitty graphics protocol; in iTerm2 and Warp through inline images; in other terminals as half-block cells. `SIGIL_GRAPHICS=kitty`, `iterm2`, or `none` overrides the detection, and tmux turns it off. A picture is 48 rows tall at most, and a picture scrolled partly off the screen shows the rows that are on it. `read` on an image reports its type and size, and the picture shows in the transcript; the model attaches it as `@path` to look at it. `/view shot.png`, or `→` on an image in the tree, shows the picture in the view pane.

A fenced `mermaid` block in a reply renders as a diagram in the transcript: flowcharts and state diagrams with box-drawing edges, sequence diagrams with lifelines and frames, pie charts as bars, and `xychart` / `xychart-beta` as labeled bars. A fenced `chart` block with `label value` rows draws the same bars. Other Mermaid kinds show their source in a frame. Flow, sequence, and pie drawings come from Sigil's `mermaid` widget, so they look the same in every terminal.

A paste of eight or more lines, or a thousand or more bytes, folds into `[pasted N lines #k]` in the draft. summonr expands every token back to the pasted text when the prompt is sent or queued; `ctrl+c` discards them with the draft.

While a turn runs, the composer status also counts waiting input (`2 queued, ↑ edits`). The terminal title reads `summonr · <workspace> · working` or `· ready`, and the terminal bell rings once when a turn ends while the terminal is not focused.

`/settings` switches tool output between compact and full, sets chain of thought to `collapsed` (the default: one dim line with a line count), `expanded`, or `off`, and sets turn receipts to `off` (the default), `collapsed` (one line: files read, searches, commands, failures, changed files, and the check result), or `expanded` (that line, the start commit, the first five failures and changed files, and each check). A click on a receipt switches between `collapsed` and `expanded`.

`/export` writes the session as markdown to `~/.summonr/exports/<id>.md`; `/export PATH` writes it where you say. It works while a turn runs. `ctrl+x y` (the default copy hotkey) copies the latest assistant reply. `/copy all` copies the full transcript. Both use OSC 52 in the terminal and the system clipboard in the window. `ctrl+x t` releases the mouse to the terminal so you can select and copy any transcript span; press it again to restore mouse interaction.

`/pane worktrees` lists the `summonr/*` worktrees of this repository: branch, commits ahead of the base, dirty or clean, the session that ran there, and the directory. Enter or `→` opens that worktree's diff (`esc` comes back); the right-click menu offers show diff, merge (the same ff-only merge a run ends with; a dirty tree is refused), remove worktree (only when clean and fully merged), and insert path. `summonr worktrees` prints the same rows; `summonr worktrees --json` as JSON. `summonr worktrees prune --json` prints the removed paths as a JSON array.

`/pane jobs` lists this workspace's background commands (the records under `.summonr/jobs/`). Each row is the command and its state (`running`, `exit N`, `killed`, or `dead`); Enter or `→` expands the job's status lines, and the right-click menu offers view log (the job's `.log` in the view pane) and kill job (the same stop `run_command` performs for `{"job": ID, "kill": true}`). The list refreshes on its own while the pane is open.

`/pane peers` lists the other live sessions on this machine: the mail roster, minus this one. Each row is `@name`, model, workspace folder, and the peer's status (`idle`, `working`). Enter or a right-click mention inserts `@name` into the prompt; a prompt that names a peer is delivered to its inbox instead of running a turn.

The card takes the keyboard (↑↓ role, enter picks a model, `d` returns it to the driving model, ←→ cycles saved workflows, `s` saves) and the mouse: click a role to pick its model, drag a role onto another to copy the model, click a saved name to apply it. The settings modal, model picker, and authentication card take clicks as well. The session log records the model behind every reply.

With a `scout` role, a `--steps` or `--goal` job opens with a read-only brief of the files, conventions, and helpers the objective touches. With a `review` role, a turn whose checks passed is read by the reviewer before it completes; a rejection sends the findings back as the next instruction and counts as a failed check.
## Questions from the model

In an interactive session the model can call `ask` to put one to four questions to you. The dialog above the composer shows one question at a time: `↑` and `↓` move between options, `space` toggles an option when several are allowed, and `enter` picks. Type in the composer to give your own answer instead, or a number to pick that option. One question sends at once; several end on a review where `y` sends, `e` edits, and `esc` cancels. A message you sent into the turn before the dialog opened never answers it; it reaches the model after the dialog closes. The same dialog shows MCP server forms. `-p`, `--steps`, `--goal`, `summonr acp`, and child sessions have no one to answer, so the model does not get `ask` there.

## Child sessions

The `task` tool delegates work to fresh child sessions. Only an interactive session offers it: `-p`, `--steps`, `--goal`, and `summonr acp` runs have no steering channel, so the model does not get `task` there. A model can emit several `task` calls in one response. summonr runs them concurrently. Each `implement` child edits an isolated checkout in `<repo>-deltas/<id>` that shares the parent's `target/` and `node_modules`. The parent applies each finished child's edits on its own. Generated files (`__pycache__`, `*.pyc`, and the pytest, mypy, and ruff caches) and summonr state under `.summonr/` other than `check.toml` and `policy.toml` never land. A child never inherits goal or steps mode: an implement child works in plain working mode. A child's first message names only its own checkout. Children started together do not see each other's edits. When two children change the same lines, that child's checkout stays on disk and the others still land. The child shows as a conflict, not a failure: `kept <repo>-deltas/<id>: conflicts with the parent`, followed by an `rm -rf` hint that removes the checkout when you are done. Task card titles use workspace-relative paths, and a card with one child takes that child's title. summonr never deletes a kept checkout. The receipt lists the files that landed. Implement children get the parent's file, shell, and language-server tools; background commands stay with the parent. The parent verifies after they finish. A `task` call may name a plugin agent with `agent`; its instructions open the child's first message, and its `tools` list limits what the child may call. The child's row names that agent. Pass `--worktree` on the session to isolate the whole run in a git worktree.

A `research` child reads the parent workspace. It cannot write files or start background commands. Its shell commands run in a read-only sandbox (Seatbelt on macOS, bubblewrap on Linux) that summonr turns on for it, so it can run checks, searches, and scripts but cannot change the workspace; where no sandbox is available it has no shell. Each child uses the endpoint context window. If the parent has a token or time budget, the children share what remains of it; otherwise a child runs until it finishes or you cancel it. A child gets the parent's system prompt and tool list byte for byte, so its first request reuses the parent's prompt cache; its instructions and working directory arrive in its first message. A child that calls `task`, `web`, an MCP tool, or peer mail gets a refusal, and a research child that calls `edit` or `write` is told it is read-only. Every child tool call, result, and answer streams into the parent transcript under the child id. A research child that finished before you pressed Esc keeps its result: the model reads it on the next turn instead of an `aborted` answer.

`task` with `background: true` starts the child and returns its id at once. `task` with `child: ID` reports its state, elapsed time, and last action while it runs, and its full result once it finishes; `kill: true` stops it. A turn does not end while a background child runs: summonr waits, then defers the turn the way it does for a background command, until the child finishes or the model kills it. **Clear** on a background status row dismisses that row for the current view without stopping the child. Its completion report and session log stay available. New children still appear.

## Tools

The model receives the full advertised schema every round (`Loop::advertised_tools`). Core names live in `src/tools/catalog.rs`.

Reads return `N:hash|text`. A stale or ambiguous tag fails before write. `read` lists a directory or shows an image. `search` finds text in files and files by name anywhere on disk. `pattern` is a regex over contents, or a list of alternatives; `name` matches files whose names hold every word in any order (`"q3 report"`); `glob` matches a file pattern, and a glob without wildcards matches names that contain it. Given together, they search the contents of the matching files only. `path` or `paths` pick where; the workspace is the default and `~` works. Case is ignored unless `case_sensitive` is true. `word` matches whole words, `literal` turns off regex, `newer` keeps files modified within a span such as `2d`, `output` picks `lines` (the default), `files`, `count`, or `folders` (matching files counted per folder), `offset` pages through long results and answers `no entries at offset N` past the end, and `depth`, `context`, and `path` given as a list narrow or widen it. A result over 50 KB is cut at a line and says how to narrow it. The older argument names `files`, `count`, `group`, `paths`, `type`, `exclude`, `before`, `after`, and `multiline` still work. Results are newest first, a name search lists symbolic links to files, and a result names the folders and gitignored files it skipped, the folders it could not read, and any path it could not open; `no_ignore` also searches gitignored files and build folders such as `node_modules` and `target`. `/proc`, `/sys`, `/dev`, and the macOS data-volume mirror are never walked. A call that runs past `search_secs` returns what it found and says so; a path that the disk never answers can still hold one background thread until the operating system returns. Overlapping paths count each file once. A name search (with `name` or `glob`) over a path outside the workspace that takes longer than `search_index_after_ms` starts a background file index of that path in `~/.summonr/cache/names/`; later name searches under it answer from the index in well under a second, check every hit on disk, and say how old the index is. An index older than `search_index_secs` refreshes in the background when it is used, so a file made since the last refresh appears after the next one. Answers from the index skip build folders unless `no_ignore`, and do not apply `.gitignore`. Search hits are `path:N:col:line`; `col` is the 1-based UTF-16 column of the match, which `lsp` takes as `character`. With `tags` true, hits are `path:N:hash|line`, and `N:hash` is the tag that `edit` takes. `edit` covers substring replace (`old`/`new`), line replace (`tag`/`new`), insert (`tag`/`text`), delete (`op: delete`), a batch (`edits`), and a rename or move (`path` with `to`). `write` creates or overwrites one file, or several (`files`); over an existing file it returns `wrote PATH (N lines, replaced M lines)`, not the diff. `read` pages by 50 KB by default; an explicit `limit` is capped at 1000 lines. `run_command` takes `workdir`; its output keeps about 50 KB of head and tail, a single line up to 8 KB, and the full log path is named only when that cut something. Only exact `cat FILE`, single-file `head`, and `sed -n A,Bp FILE` without a `workdir` are answered by `read`. Plain search commands without a `workdir` are answered by `search`: `grep` and `rg` with common flags (`-r`, `-n`, `-i`, `-l`, `-c`, `-w`, `-F`, `-e`, `--include`, `-g`, `-C`), `find PATH -name|-iname GLOB` with `-maxdepth`, `-type f`, and `-not -path`, and `mdfind -name WORDS [-onlyin DIR]`, alone, joined by `;`, or piped into `head`. The result names the search call it ran. Any other form, and `ls`, `wc`, and `tree`, run in the shell. A foreground command waits up to two minutes; past that it keeps running as a background job and returns its id and the call that waits for it, unless `timeout_secs` was set, which stops it instead. Where a command cannot become a job (a child session, a read-only research child, a restricted write policy, or the jobs module turned off), it is stopped after an hour. Explicit and automatically backgrounded commands keep running after summonr exits, with logs and exit status under `.summonr/jobs/`. `background: true` cannot be combined with `timeout_secs`; use a foreground timeout or stop the returned job. Foreground commands own their descendants: cancellation and completion stop the process group. A background shell waits for its shell jobs before recording completion. A job poll waits until the job exits or the wait limit passes.

| Name | When | Notes |
| --- | --- | --- |
| `read` | always | File, directory, or image |
| `search` | always | Contents (`pattern`) or files by name (`name`, `glob`) under any readable path; ignores case; `newer`, `group`, `offset`, `word`; content search skips files over 1 MiB |
| `edit` | always | Change or move a file; hashes from `read`; `edits` for a batch |
| `write` | always | New or empty files; `files` for several |
| `run_command` | always | Sandbox; `workdir` / `background` / `job` / `kill`; full log path |
| `todo` | `/steps`, `/goal`, `--steps`, `--goal` | Replaces the Work todo list; status `pending`, `active`, or `done` |
| `skill` | skills catalog exists | Loads one `SKILL.md` by name |
| `lsp` | `lsp` on | Diagnostics for a `path`; definition, `references`, or `new_name` at `line` and `character` |
| `web` | `search` on (off by default) | `query` searches; `url` fetches as markdown, text, or html |
| `browser` | `browser` on (off by default) | One Charm action as JSON, or `snapshot`, `read`, `screenshot`, `tabs`, `login`, `close` |
| `task` | `task` module, interactive session | Child session |
| `ask` | interactive session, not in a child | One to four questions; each has up to four options (`label`, `description`) and `multiple`, or no options for a typed answer |
| `agents` | `mail` module | List live agent sessions by name |
| `send` | `mail` module | `to` (a name, an id, or `all`) and `text`: write to a peer's inbox |
| `inbox` | `mail` module | Read mail that arrived since the last read |
| `board` | `mail` module | `action` get, set, or list; `key`, `value` |
| `mcp` | an MCP server has over 8 KB of tool schemas | Lists those servers' tool names; `query` returns matching tools with schemas; `tool` and `arguments` call one |

The old names (`read_file`, `list_dir`, `grep`, `glob`, `show_image`, `write_file`, `replace_text`, `replace_lines`, `insert_after`, `delete_lines`, `replace_texts`, `edit_file`, `diagnostics`, `definition`, `web_search`, and `web_fetch`) still execute and are not advertised. A read-only research child is offered `read`, `search`, and `run_command`, plus `skill` and, with `search` on, `web`; not `edit`, `write`, or `todo`.

## Modules

Every optional capability is a **module** compiled into the binary: `search`, `browser`, `lsp`, `jobs`, `task`, `skills`, `mcp`, `mail`, and `memory`. That list is the built-in module set: curated, switchable, and inside the harness process RSS. `search`, `browser`, and `mail` are off by default so unused tool schemas stay out of the first `prefix_hash`; `mcp` is on, and adds schemas only for servers you configure; the mail roster and `@` messages work without the `mail` module. `summonr modules` lists them with their state; the Modules rows in `/settings` in the interface switches one and writes `[modules]` in `~/.summonr/config.toml`. After the first `prefix_hash` in this process, that write is for the next process only. A workspace `.summonr/config.toml` may turn a module off for that project but cannot turn one on. A new compiled tool is a row on [`module-api.md`](internals/module-api.md) in the same change; if a skill or MCP can do the job, it is not a module. summonr runs no third-party code inside the harness; modules read declarative files under `.summonr` where they need user input.

The `search` tool is always on and is separate from the `search` module. It honors `.gitignore`, skips build directories, binaries, and files over 1 MiB, and lists matching files when only `glob` is set. With the `search` module on, `web` with `query` reads DuckDuckGo's results page, then Brave's when DuckDuckGo returns nothing, without a key, and `url` fetches one page. `freshness` takes `day`, `week`, or `month`. The engine list is data in `src/web/engines.toml`. Both go through Leyline with a browser identity, so no search API key or self-hosted search service is needed. `[search]` takes `count`, `allowed_domains`, and `excluded_domains`.

With the `browser` module on, the `browser` tool drives the Chrome, Edge, or Brave already installed, through Charm. It takes one action as JSON (`navigate`, `click`, `fill`, `press`, `select`, `check`, and the rest of Charm's actions, with targets as `eN` refs from a snapshot or CSS), plus `snapshot`, `read`, `screenshot`, `tabs`, `login`, and `close`. It uses a profile of its own at `~/.summonr/browser/profiles/NAME`, never your everyday browser, so it holds only the logins you give it; `login` opens a visible window where you sign in. Cookies are saved after every browser call, so a login survives the next session even when summonr exits abruptly. Every URL it opens passes the same check as `web`: no private hosts and no plain `http`. When `web` with `url` meets a page that needs JavaScript, it renders the page in this browser. `[browser]` takes `profile` (the profile name, `default` when unset), `headed`, `chrome` (the executable), and `events` (`page` and `runtime`). Language modules cover Rust, TypeScript/JavaScript, Python, and Go. They select the server from the file and a project marker, then keep one connection per module in a polyglot checkout. Rust passes initialization options that disable check-on-save, build scripts, proc macros, and `cargo --all-targets`. Explicit Cargo commands still run through `run_command`.

A workspace `.summonr/config.toml` may force a built-in module:

```toml
[lsp]
module = "rust"
```

Repository config cannot supply executable argv. A trusted `~/.summonr/config.toml` may use `[lsp] command = ["server", "--stdio"]` when a built-in module does not fit. Language servers run as trusted host processes with the inherited environment, outside the command sandbox. They can access files and network resources with your account's permissions. Rename edits returned to summonr still pass through its file policy. Disable the `lsp` module when you do not trust the server or the project configuration it reads.

Unknown flags exit 1. A red verify gate exits 2. In `--goal` and `--steps`, the check decides: when a run stops short, summonr runs the configured check once at exit, and a failed check exits 2 even when the budget ran out. A run that stops short exits 3 when no check is configured, or when the check passed but the goal was not met: a budget breach, a stalled or impossible goal, or an aborted turn. `-p` runs no check at exit. On Unix, `SIGINT` exits 130, `SIGTERM` 143, and `SIGHUP` 129; on Windows, Ctrl+C exits 130. A signal exit skips the `--worktree` finish, so the worktree and its branch stay in place. JSON turn receipts keep model text separate from the changed-path list and every configured completion check. `verified` on a receipt is true only when at least one check ran and every check passed; a turn with no configured check completes with `verified: false`.

## Mail

Agent sessions on this machine share a mailbox: files under `~/.summonr/mail`: a roster, one inbox per session, and one board per workspace. There is no server; each session appends and polls files. Mail is peer text, never user instructions, and each session's own permissions still apply.

Every summonr session joins the roster under a name: the one you set with `--name` or `/name`, or else the workspace folder and a number, such as `summonr-1`. Child sessions stay off the roster, and `SUMMONR_MAIL=off` keeps a session off it. `/agents` lists the sessions on the roster.

To message a session, type `@` in the composer and pick it from the list, or type its name, such as `@summonr-2 the schema landed`. A prompt that names a live session goes to that session's inbox instead of to the model. The mail arrives there as `[mail from @summonr-1 …]`; `[agent] mail` in settings decides whether it starts a turn or waits for the next one. Reply the same way, with `@summonr-1`.

The `mail` module, off by default, also gives the model the tools `agents`, `send`, `inbox`, and `board`. `send` takes a name, an id, or `all`.

The same mailbox is a CLI, so any agent CLI (Claude Code, Codex, a script) can take part without a server:

```
summonr mail                  # who is on this machine
summonr mail send <name|id|all> <text>
summonr mail inbox <id> [--clear]
summonr mail board [get <key>|set <key> <text>]
```

A foreign agent takes part by picking an id: it reads `summonr mail inbox <id>`, and anyone can write to it with `summonr mail send <id> <text>`. Its messages arrive from `cli` unless it writes its own inbox name into replies. `inbox <id> --clear` prints the mail and empties that inbox. It refuses the inbox of a live summonr session, because that session keeps its own read position. A lock file next to each inbox makes a send, a read, and a clear wait for each other, so no message is lost between them.

## Memory

summonr remembers across sessions through two Markdown files that load into every trusted session: user memory at `~/.summonr/memory/MEMORY.md`, and project memory at `~/.summonr/memory/<name>-<hash>/MEMORY.md`, where `<name>` is the repository folder and `<hash>` comes from its first commit so that a rename or clone keeps it. Every worktree and child checkout of one repository shares its project memory; another repository never sees it. Ask the model to remember something and it writes the file with its normal edit tools, one dated entry per fact. It is told to save corrections, stable preferences, and facts the code does not show, never what the repository already records, and to check a remembered fact against the code before it acts on it. Edit or delete the files yourself at any time; `/memory` shows where they are, how full they are, and whether memory is on. Changes apply to new sessions. Project memory follows the repository, not its folder: moving, renaming, or cloning a repository keeps its memory, because the key comes from the repository's first commit (a folder with no commits is keyed by its path).

Each file loads up to `memory_bytes` (25,000 bytes). A longer file loads its newest entries, under a note that tells the model the older part is not shown and to read the whole file before it edits it, so a prune never deletes what the model did not see. Invisible and direction-changing characters are dropped, and secret-shaped values become `[secret]` before the text reaches the model. Memory loads only in a trusted workspace (`--no-trust` skips it), and `memory = false` under `[modules]` turns it off. Under `--sandbox`, the summonr home is protected, so the model reads memory from its prompt but cannot write it.

`summonr sessions --grep TEXT` searches the text of past sessions, newest first, case-insensitively, and prints the session id, its start time, its directory, and each matching line; it exits 1 when nothing matches. The model can run it through `run_command` to recall earlier work.

## Cron

`summonr cron` runs a prompt or a goal on a schedule, headless, in the directory you add it from:

```sh
summonr cron add nightly --cron "0 3 * * 1-5" --goal "make cargo test pass"
summonr cron add digest --every 6h -p "summarize what changed in git log since yesterday" --mail summonr-1
summonr cron add once --at 09:00 -p "check the release notes"
summonr cron add triage --manual -p "triage the alert in the payload"
summonr cron install
```

A schedule is five cron fields (minute, hour, day, month, weekday, with `*`, lists, ranges, steps, and `jan`-`dec` and `sun`-`sat` names; `0` and `7` are Sunday), `@hourly`, `@daily`, `@weekly`, `@monthly`, or `@yearly`, `--every` with `m`, `h`, or `d`, one `--at` time (`HH:MM` for the next such time, `YYYY-MM-DD HH:MM`, or RFC 3339; a time already past is refused), or `--manual` for a job that runs only through `summonr cron run` or the webhook. When both day and weekday are set, either one matches. Times are local, or in `--tz ZONE`. A time that daylight saving skips does not fire that day, and a repeated time fires once. `--every 1d` fires at midnight and `--every 2h` on even hours. An `--at` job pauses itself after its scheduled run; running it by hand or from the webhook does not use it up.

`summonr cron install` asks the OS to run `summonr cron tick` once a minute: a launch agent on macOS, a systemd user timer on Linux, and a scheduled task on Windows. A launch agent runs while you are logged in; when launchd refuses one, as on a Mac with no one signed in, summonr installs a crontab line instead, and cron then needs Full Disk Access for jobs that touch Desktop, Documents or Downloads. A systemd user timer stops at logout unless lingering is on (`loginctl enable-linger`); install says so, and falls back to a crontab line when systemd has no user session. On macOS and Linux it records your `HOME`, `PATH`, and `SUMMONR_HOME`, so jobs load the same user instruction files and tools as the shell you installed from; on Windows, set them as user environment variables. Store keys with `summonr login` rather than in your shell. `uninstall` removes it, and `status` says whether it is installed and when the next job fires. No summonr process stays running.

Each due job starts as its own process with `--json` and a time budget: `--max-secs`, or `cron_budget_secs` (30 minutes). `--endpoint`, `--model`, `--max-tokens`, and `--worktree` pass through. A `--goal` job is done only when the workspace check passes. A job still running when its next time comes is skipped, and that skip is logged. When the machine was asleep, the latest missed time runs once; times older than `cron_catchup_secs` (7 days) are logged as expired. After `cron_fail_pause` failures in a row (3), the job pauses itself; `summonr cron resume NAME` restarts it. A run that cannot start, for example because its directory is gone, counts as a failure. An `--at` time that passed while the machine was off past the catch-up window is logged as expired and the job pauses. A goal cannot start with `-`. A goal runs until the judge agrees it is met; a precise goal ("make `cargo test` pass") ends sooner than an open one.

`--notify failure` (the default) shows a desktop notification when a run fails, `always` after every run, and `never` stays quiet. `--mail SESSION` also sends the result to that session's inbox, and `--chat` to every chat paired with `summonr gateway`. `summonr cron log NAME` lists past runs with their outcome, then the last run's final answer and verdict ("check passed", "check failed", or why it stopped) and the path of its full log; `--raw` shows the raw events. The run itself is a job under `.summonr/jobs` in the job's directory. `cron add` and `cron show` name the endpoint a run uses: the one set with `-e`, or the one summonr would pick now, with the reason. `cron add` warns when that endpoint has no stored key (cron runs do not see your shell's variables), when `--mail` names no live session, and when `--chat` has no paired chat. `summonr cron run NAME` runs a job now. Jobs live in `~/.summonr/cron/jobs.toml`. The model manages jobs with the same commands through `run_command`, except inside a cron run, which cannot add, edit, remove, pause, resume, run, install, or uninstall jobs, or print a token.

## Gateway

`summonr gateway` lets a chat app or a web request start work on your machine. Nothing listens on a public port.

### Telegram

Create a bot first: open [@BotFather](https://t.me/BotFather) in Telegram, send `/newbot`, pick a display name and a username ending in `bot`, and copy the token it replies with (it looks like `123456789:AA...`). Then:

```sh
summonr gateway setup telegram     # paste the bot token; it is stored, never shown
summonr gateway telegram           # run the bridge here; Ctrl+C stops it
```

The bridge runs while its terminal runs. To keep it going after you close the terminal, start it in `tmux` or `screen`, or as a launch agent or systemd user service that runs `summonr gateway telegram --dir PROJECT`. Pair your account before you leave the machine: pairing needs a command on the machine.

The bridge asks Telegram for new messages, so it works behind a router or firewall. The bridge answers only private chats, never groups or channels. The first time you message the bot it answers with a pairing code, and the terminal shows who asked; run `summonr gateway pair CODE` on the machine to allow your Telegram account. Until you pair them, other people who message the bot get only a pairing code they cannot use, and nothing else. Codes expire after `gateway_pair_secs` (an hour). `summonr gateway status` lists paired accounts with their Telegram user ids, and `summonr gateway unpair USER_ID` removes one, with its chat and any messages it queued.

If you lose the phone or a paired account is compromised: stop the bridge (Ctrl+C, or stop its tmux session or service), run `summonr gateway unpair USER_ID`, revoke the token in @BotFather (`/revoke`), and run `summonr gateway setup telegram` with the new token. The old token stops working at once.

Each chat is one summonr session in the directory where the bridge runs (`--dir` picks another; `--endpoint` and `--model` pick the seat). Every message is one turn, and the answer comes back to the chat. Messages sent while a turn runs wait for the next turn, and the bot says how many are ahead. `/new` starts a fresh session, `/status` shows the current one, `/stop` ends the running turn (it stops the turn's commands too, then kills what is left after `gateway_stop_secs`), and `/start` or `/help` shows these commands. A reply says when the check failed or the run broke. The bridge remembers where it stopped reading, so a restart never runs a message twice; a second bridge on the same bot is refused. A chat turn runs like `-p` with your account: it can run any command your policy allows and read or write anything you can, unless you contain it. Anything that needs an approval is denied. Pair only accounts you trust with that directory, start the bridge in a project folder rather than your home, and consider `on = true` under `[sandbox]` in `~/.summonr/config.toml`.

`summonr cron add ... --chat` also sends a job's result to every paired chat. The token is kept where summonr keeps API keys (`~/.summonr/credentials.toml`, or the keychain with `secrets = "keychain"`); `summonr gateway setup telegram` again replaces it. The `gateway_*` settings are limits: override them under `[limits]` in `~/.summonr/config.toml`.

### Webhook

```sh
TOKEN=$(summonr cron token nightly) # the token on stdout, how to use it on stderr
summonr gateway webhook            # listens on 127.0.0.1:8765
curl -X POST -H "Authorization: Bearer $TOKEN" --data @event.json http://127.0.0.1:8765/run/nightly
```

`POST /run/NAME` with the job's token starts that cron job now and answers `202` with `{"job", "slot", "log"}`: `slot` is the run's start time in Unix seconds, the time `summonr cron log NAME` shows in its first column, and `log` is that command, which shows how it went. A job you only fire from outside is a `--manual` job. A wrong token or an unknown job both answer `401` before any body is read, a second webhook fire within `gateway_fire_secs` (60 seconds) answers `429` (scheduled runs and `cron run` do not count), a body over `gateway_body_bytes` (64 KiB) answers `413`, a chunked body answers `411`, and a request not finished within `gateway_read_secs` answers `408`, another method answers `405`, and a paused job answers `409`. Every error has a JSON body `{"error": ...}`, and `429` carries `Retry-After`. A query string is ignored. Up to 32 requests are served at once, and a client that has not sent its headers within `gateway_head_secs` (2 seconds) is dropped, so slow clients cannot hold up real events. The listener prints one line per request (method, path, status, and job) to stderr, never the token or body, and Ctrl+C, `SIGTERM`, or `SIGHUP` stops the listener. The body is saved in the job's directory under `.summonr/jobs`, the run is told it is untrusted event data, not instructions, and it runs with `--sandbox`, so it cannot write outside its directory or touch the summonr home and memory. Saved bodies are deleted with the run history that refers to them. What the run read from the body stays in that run's log under `.summonr/jobs` and in its session under `~/.summonr/sessions` until those are pruned (`[sessions] retain_days`), so do not send secrets you would not keep in a log. The listener binds loopback only; to take events from another machine, put a tunnel or reverse proxy in front of it. `summonr cron token NAME` again rotates the token, and `summonr cron remove NAME` forgets it.

### GitHub

GitHub Actions is already a listener for issues and comments. This workflow runs summonr when someone with write access comments `/summonr <task>` on an issue, then opens a pull request:

```yaml
name: summonr
on:
  issue_comment:
    types: [created]
permissions:
  contents: write
  pull-requests: write
jobs:
  run:
    if: startsWith(github.event.comment.body, '/summonr ') && !github.event.issue.pull_request
    runs-on: ubuntu-latest
    steps:
      - name: Require write access
        env:
          GH_TOKEN: ${{ github.token }}
          ACTOR: ${{ github.event.comment.user.login }}
        run: |
          level=$(gh api "repos/$GITHUB_REPOSITORY/collaborators/$ACTOR/permission" --jq .permission)
          case "$level" in admin|write) ;; *) echo "$ACTOR has $level access"; exit 1 ;; esac
      - uses: actions/checkout@v4
        with:
          persist-credentials: false
      - run: curl -fsSL https://summonr.dev/install.sh | sh
      - run: sudo apt-get update && sudo apt-get install -y bubblewrap
      - name: Work on the request
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
          BODY: ${{ github.event.comment.body }}
        run: |
          task=$(printf '%s' "$BODY" | perl -0pe 's/<!--.*?-->//gs; s/^\/summonr //')
          ~/.local/bin/summonr anthropic --goal "$task" --sandbox --json --max-secs 1800 > "$RUNNER_TEMP/summonr.jsonl"
      - name: Open a pull request
        env:
          GH_TOKEN: ${{ github.token }}
          ISSUE: ${{ github.event.issue.number }}
        run: |
          gh auth setup-git
          git switch -c "summonr/issue-$ISSUE-$GITHUB_RUN_ID"
          git add -A
          git -c user.name=summonr -c user.email=summonr@users.noreply.github.com commit -m "Work on #$ISSUE"
          git push -u origin HEAD
          gh pr create --title "Work on #$ISSUE" --body "From the comment on #$ISSUE."
```

The comment reaches the workflow through an environment variable, never pasted into the script, and hidden HTML comments are stripped before the model sees it. Only people with write or admin access to the repository can start it; the first step asks GitHub for the commenter's permission. The checkout keeps no GitHub token, so the run holds only the model key and its shell is sandboxed; the pull request step signs in after the run ends. The repository needs a `.summonr/check.toml` (for example `check = "cargo test"`): without one, `--goal` stops with exit 3 and no pull request opens, and when the check fails it exits 2. Comments on pull requests are ignored, and each run uses its own branch.

## Limits

Everything in this section describes a contained session. Without `--sandbox`, or `on = true` under `[sandbox]`, none of it applies: the shell is uncontained and the command and file policy is off.

Two protections hold in every session, contained or not. `search` never reads a protected file or folder. Secret values never reach the model: before a tool result is shown or sent, summonr replaces with `[secret]` every value that looks like a secret in the workspace's `.env` and `.env.*` files (three folders deep), in `.npmrc` and `.pypirc`, in `~/.aws/credentials`, `~/.netrc`, `~/.npmrc`, `~/.pypirc`, `~/.git-credentials`, `~/.config/gh/hosts.yml`, and `~/.docker/config.json`, and every PEM private key block. A value looks like a secret when its key names one (`KEY`, `SECRET`, `TOKEN`, `PASSWORD`, `AUTH`, and similar words, in `KEY=value`, `key: value`, or JSON form), when it is a URL with a password, or when it is a long mix of letters and digits. Commands still read the real files, so a test suite that loads `.env` runs as before.

Each flag alone gives these modes:

| Mode | File tools | Shell commands | Shell network |
| --- | --- | --- | --- |
| Default | Full access with your account's permissions | Uncontained; command policy bypassed | Your account's access |
| `--sandbox` | Reads any path the sandbox can read; writes under the workspace and extra writable roots | Contained; workspace, extra writable roots, and temporary storage writable | Allowed, or denied with `--no-net` |

`read` and `search` reach any path except the protected ones, and the summonr home is protected wherever `SUMMONR_HOME` puts it. A command that reads a protected path is refused from any `workdir` or after `cd`. `edit` and `write` stay inside the writable roots: the launch directory, `[sandbox] write` in `~/.summonr/config.toml`, and `--add-dir`. For work that spans sibling repositories, put the parent on that list, launch summonr in the parent directory, or pass `--add-dir`.

Commands use Seatbelt on macOS and bubblewrap on Linux. Windows AppContainer is unfinished and needs a build with `--features windows-sandbox`; Git for Windows and user-installed toolchains stay blocked. A missing backend is a hard stop. Sandboxed sockets are allowed by default. `--no-net` or `/permissions net off` denies them. That flag does not change file permissions. Model requests and language servers use host networking separately from shell commands.

`/permissions net on` allows sockets for the rest of the session, `/permissions net once` allows only the next command, and `/permissions net off` denies again. The interface also prompts when a sandboxed command fails with a network error: `[ allow ]` turns the session network on and retries the command; `[ deny ]` returns the failure to the model. Without an interactive approval channel, the failure returns to the model and network stays denied. `/permissions` alone reports the network and write state.

An OS sandbox failure that cannot be resolved by a write-root or network grant requires interactive approval to run that command without the sandbox. Without that approval, the command stays contained. Approval of a destructive command keeps the sandbox and its network setting; running it without containment requires a separate approval. A read-only research child cannot run outside the sandbox.

Auto mode asks before it grants a write outside the writable roots, then retries the tool. Without an interactive channel the write stays denied. It does not write `~/.summonr/config.toml`. `/permissions write PATH` persists to that list without a prompt. `/permissions write --session PATH` grants for this session only. `/permissions write` alone lists the extra roots. Writers such as `cp`, `ln`, `mv`, `mkdir`, `touch`, and `install` may then target those paths, and the OS sandbox allows the write. `write` and `edit` accept files under them too. `~` and `$HOME` expand before the check. The same irreversible commands ask before they run, with or without the sandbox. The prompt shows the whole command, wrapped, and says why it needs approval, as `<command> — <reason>`. Under `--strict` they never run, and neither do force pushes, `sudo`, pipes into a shell, history rewrites, or writes to protected paths. Under `--sandbox` or `--strict`, matches on your own `[policy] deny` list are refused without a prompt. In the sandbox, a write to a protected path (`.git/hooks`, `.git/config`, `.git/HEAD`, `.git/index`, `.git/refs`, `.git/logs`, `~/.ssh`, `~/.gnupg`, `~/.summonr`, shell startup files, `.env`) asks before it is granted. Without an interactive channel it stays denied. A protected file inside the workspace stays denied. Reads of `.env` and `.env.*`, files named `credentials` or `credentials.*`, `.netrc`, `.git-credentials`, and `.pypirc`, and the home folders `.ssh`, `.gnupg`, `.aws`, `.summonr`, `.docker`, `.kube`, `.config/gh`, and `.config/gcloud` are blocked, and so is a shell content search such as `grep -r` or `rg`, or a copy, over a folder that holds one of those directories. A recursive shell search of a folder that holds a `.env` still runs, and the secret values in what it prints are replaced.

A sandboxed `cargo`, `rustup`, `npm`, `npx`, `pnpm`, `yarn`, `bun`, or `go` command that fails with `Operation not permitted` offers the matching home directory (`~/.cargo`, `~/.npm`, or `GOPATH` / `~/go`) as the grant.

Containment is opt-in: `--sandbox`, or `on = true` under `[sandbox]` in `~/.summonr/config.toml`. An uncontained session writes anywhere your account can and bypasses command and file policy. `--no-sandbox` is accepted and changes nothing. `--strict` reapplies policy restrictions but does not restore process containment.

A build tool that writes outside the writable roots fails inside the sandbox with `Operation not permitted`. Name those directories in `~/.summonr/config.toml` (or pass `--add-dir`) and every contained command, including a research child's, can write there:

```toml
[sandbox]
write = ["~/.cargo/registry", "~/code"]
```

A `~/` prefix expands to your home directory. A listed path that does not exist is skipped at startup; it is still a writable root for policy checks.

A provider reply of 429, a 5xx status, or a connection error is retried up to three times with a wait that doubles from 1 s to 8 s. A `Retry-After` header replaces that wait; one over 60 s stops the retries so the turn can fall back to the next endpoint. A reply that already streamed output is retried once, then the turn falls back. A rate limit, or a connection failure before any output, also gets one more completion after those retries, unless its `Retry-After` is over 60 s.

A language server gets 30 s to start and 30 s to answer an `lsp` call. The diagnostics check after an edit waits 5 s and is skipped when the server is slower. A server that misses a deadline keeps running, so a slow first index can finish in the background.

summonr does not steer the model. Tool results reach it unchanged, and it decides when a turn is done; only a token or wall-clock budget that you set ends a turn early. A reply cut off at the output limit gets one note saying so. The compaction summary request sends the session's tools and history unchanged, so it reuses the prompt cache; a failed summary retries after 8 rounds.

User policy is `~/.summonr/policy.toml`; repository policy is `.summonr/policy.toml`. A nonempty command allow list must match on every overlay (intersection). Deny lists accumulate. `write` globs in policy.toml are a narrowing filter on paths relative to a writable root; extra roots themselves come only from `[sandbox] write`, `--add-dir`, and `/write`. Empty lists add no restriction, except that `strict = true` with an empty allow list denies all commands. Repository policy can narrow user permissions, never widen them. Children inherit these restrictions.

`confirm` picks which commands ask first. `confirm = "destructive"`, the default, asks only before irreversible destruction (see Confirmation). `confirm = "risky"` also asks before `sudo`, force pushes, pipes into a shell, git commands that discard work or rewrite history, and writes to protected paths. The stricter value wins across overlays. Shift+Tab toggles this session between auto approval and approval for risky commands. Auto approval still asks before destructive commands and configured `ask` patterns. The toggle cannot lower a configured `confirm = "risky"` or relax strict mode, command denies, or write restrictions. The composer labels approval separately from auto, work, and goal execution modes. During a turn, the change is queued until the turn ends and the active label stays unchanged.

```toml
confirm = "risky"
```

`ask` lists commands that always ask first, whatever `confirm` says. A command matches when it contains a pattern, the same rule as `deny`. A match shows `[ allow ]` / `[ deny ]`, and under `-p` it does not run. User and repository lists add together; a repository can add patterns, never remove them. Strict mode does not read `ask`.

```toml
ask = ["git push", "git tag", "cargo publish", "npm publish", "gh release"]
```
In a read-only research child, `edit` and `write` are denied and shell commands stay read-only. A restricted write list makes contained shell commands read-only and disables background commands; file tools still use the list.

## Extend

The built-in modules are the binary (modules above). What you add lives in files and other processes so the harness RSS and the frozen prefix stay small. Other products name that package a plugin; summonr does not load guest code. [Extending summonr](extending.md) is the task page.

| Layer | Where | What the model sees |
| --- | --- | --- |
| Skill | `~/.summonr/skills/<name>/SKILL.md` or `.summonr/skills/<name>/SKILL.md` | One `skill` tool. The body is a later tool result. |
| MCP | `[mcp.NAME]`; approved `.mcp.json`; `import_mcp = true` for Claude/Codex configs | `server__tool` aliases that connected before the first `prefix_hash`; a server over 8 KB of schemas is reached through one `mcp` tool. `/server:prompt` and `@server:uri` expand server prompts and resources into the user message. |
| Hook | `[hooks]` `pre_tool`, `post_tool`, `compact`, `stop` (a command, a table, or a list) | Not a tool. `pre_tool` may block a call (stdin is JSON). |
| Plugin | `summonr install PATH` (Claude Code layout) | Its skills, MCP tools (`<plugin>-<server>__tool`), hooks, and `bin/` |
| CLI | `run_command` | A binary on `PATH` or under `~/.summonr/bin/<name>/`. Policy, danger prompt, and `--sandbox` apply. Copying files must not execute the binary. Workspace `.summonr/bin` is not put on `PATH` unless the user approved it the same way as `.mcp.json`. |

`summonr install PATH` copies a plugin directory to `~/.summonr/plugins/<name>/`. Copy does not execute. Restart so new MCP aliases join the next `prefix_hash`. `summonr install --list` prints one line per plugin: its name, on or off, and what it adds (skills, MCP servers, commands, agents, language servers, hooks, `bin`). There is no marketplace and no in-process loader.

After the first `prefix_hash`, advertised tools and skill catalog names stay frozen until process exit. A module switch in `/settings` persists for the next process and does not rebuild this session. Hub reconnects that would change aliases are ignored. Plan vs work may still drop `edit`/`write`, and a steps or goal run adds `todo`.

When the workspace is trusted, summonr injects `~/.agents/AGENTS.md`, `~/.claude/CLAUDE.md`, and `~/.config/devin/AGENTS.md` as user context, then `AGENTS.md` and `CLAUDE.md` at the workspace root as project context. A file that resolves to one already loaded is skipped, and a section repeated across files is sent once. A line in one of these files that starts with `@` names another file relative to it, and its text joins the prompt in place; each file loads once, and a missing include stays as the literal line.

`~/.summonr/SYSTEM.md` replaces the built-in preamble when that file is nonempty. `<workspace>/SYSTEM.md` is appended after it, and only when the workspace is trusted. `SUMMONR_SYSTEM_PROMPT` still replaces the whole prompt.

### Hooks

Each of `pre_tool`, `post_tool`, `compact`, and `stop` under `[hooks]` takes a command string, a table `{ command = "...", dialect = "claude" }` or `{ command = "...", dialect = "summonr" }`, or a list of either. A list runs in order. `dialect` defaults to `summonr`. A table can add `matcher`, with the syntax of a Claude Code `hooks.json` matcher: tool names separated by `|`, or a regex. The hook then runs only for a tool whose name matches, in the hook's dialect (`run_command` in `summonr`, `Bash` in `claude`). In `claude`, `search` is `Grep`, or `Glob` for a name search with no `pattern`, and either matcher matches every search. A hook on `search` does not see `grep` or `rg` run through `run_command`; to block both, also match `run_command` or add the commands to `[policy] deny`.

```toml
[hooks]
pre_tool = "./scripts/guard.sh"
stop = [{ command = "./scripts/notify.sh" }, { command = "claude-hook.sh", dialect = "claude" }]
```

A `summonr` hook reads this JSON on stdin. Every event also carries `session` and `workspace`:

| Event | Fields |
| --- | --- |
| `pre_tool` | `event`, `id`, `tool`, `arguments` |
| `post_tool` | `event`, `id`, `tool`, `arguments`, `success`, `output` |
| `compact` | `event`, `summary` |
| `stop` | `event`, `text`, `aborted` |

Exit 0 allows. Any other exit blocks a `pre_tool` call, and stderr is the reason; the rest of the list does not run. For the other events a nonzero exit is a note.

A `claude` hook gets Claude Code's stdin JSON: `session_id`, `cwd`, `hook_event_name`, and for tool events `tool_name`, `tool_input`, `tool_use_id`, and `tool_response`. Exit 2 blocks a `pre_tool` call, and so does exit 0 with `hookSpecificOutput.permissionDecision = "deny"` on stdout. Any other nonzero exit is a note. Plugin hooks use this dialect.

Every hook gets `SUMMONR_HOME` and `CLAUDE_PROJECT_DIR` in its environment. A plugin hook also gets `CLAUDE_PLUGIN_ROOT` and `SUMMONR_PLUGIN_ROOT`, and `CLAUDE_PLUGIN_DATA` and `SUMMONR_PLUGIN_DATA`.

Intent permissions: a `pre_tool` hook that exits nonzero blocks the call. Keep argv policy in `policy.toml`; do not add a second classifier unless it is a compiled module row.

Remote TUI: `summonr acp`, or `ssh` and the terminal. There is no HTTP control plane.

Browser: the `browser` module drives Chrome through Charm. A Playwright MCP server also works, as any MCP server does.

`summonr export` is Markdown of `~/.summonr/sessions/<id>/session.jsonl`. It does not import transcripts from other tools.

Search is the `search` tool, or `rg` through `run_command`. A 1k-token impact graph is a named MCP server, not a second search tool.

## Theme

`theme` in `config.toml` selects `space` (the default), `nord`, `dawn` (a low-glare light theme), or `auto`, which picks `space` or `dawn` from the terminal's background. The Theme row in `/settings` cycles them. `background = "terminal"` leaves the background unpainted so the terminal's own background (and any transparency) shows through; `"theme"`, the default, paints the theme's `base` color. The Background row in `/settings` switches between them. Every other color is a named token you can override in a theme file:

```toml
accent = "#698cd2"
background = "#0c0c14"
code_bg = "#1e202a"
```

Copy [`theme.example.toml`](../../theme.example.toml) to `~/.summonr/theme.toml`, or to `<workspace>/.summonr/theme.toml` for one project. Unset tokens keep the preset. Values are `#rgb`, `#rrggbb`, or `#rrggbbaa`. A file that does not parse, an unknown token, or a bad color is reported, and that token keeps the preset. Both files apply on top of the chosen theme. The tokens are `base`/`background`, `text`/`foreground`, `gray`, `dim`, `thought`, `accent`, `green`, `red`, `error`, `yellow`, `cyan`, `black`, `user_bg`, `tool_bg`, `code_bg`, `diff_add_bg`, `diff_del_bg`, `code_comment`, `code_string`, and `code_keyword`.

## Herdr

When `summonr` runs inside a [Herdr](https://herdr.dev) pane, it reports lifecycle state through Herdr's CLI (`pane report-agent`). `idle` is the prompt; `working` is a turn. The summonr session id is the native session reference. `/name TEXT` becomes the pane title and `$summary` token. Child sessions do not report, so a nested `task` does not steal the pane. Herdr has no built-in `summonr` kind. This reporter is the integration.

## Docs

- [Work](architecture.md)
- [`config.example.toml`](../../config.example.toml)
- [`policy.example.toml`](../../policy.example.toml)
