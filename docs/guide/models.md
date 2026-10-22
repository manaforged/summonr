# Models and sign-in

This page helps you connect summonr to a model: a provider key, a coding plan, or your own server.

## Pick a provider

Find your provider below and run `summonr NAME` with the name in its row, for example `summonr openai`. A provider with several routes, such as a coding plan and an API key, or an international and a China region, has one row per route. Every row takes `--model` to pick another model.

| Provider | Name | Pay with | Key variables, first set wins | Default model |
| --- | --- | --- | --- | --- |
| Anthropic | `anthropic-api` | API key | `ANTHROPIC_API_KEY` | `claude-opus-5-5` |
| Anthropic | `claude` | Claude Pro or Max, through your Claude Code | none | Claude Code's |
| OpenAI | `openai` | ChatGPT Plus or Pro sign-in, or API key | `OPENAI_API_KEY` | `gpt-6-astra` |
| xAI | `grok` | SuperGrok or X Premium+ sign-in, or API key | `XAI_API_KEY`, `GROK_API_KEY` | `grok-4.7` |
| Google | `gemini` | API key | `GEMINI_API_KEY`, `GOOGLE_API_KEY` | `gemini-3.8-flash` |
| GitHub Copilot | `copilot` | Copilot plan, GitHub sign-in | none | `gpt-5.3-codex` |
| DeepSeek | `deepseek` | API key | `DEEPSEEK_API_KEY` | `deepseek-flash` |
| Z.ai (GLM) | `zai-intl` | Coding plan, international | `ZAI_CODING_API_KEY`, `ZAI_API_KEY`, `ZHIPU_API_KEY`, `GLM_CODING_API_KEY`, `GLM_API_KEY` | `glm-5.3-flash` |
| Z.ai (GLM) | `zai-cn` | Coding plan, China | `ZAI_CODING_CN_API_KEY`, `GLM_CODING_API_KEY`, `GLM_API_KEY`, `ZAI_CODING_API_KEY`, `ZAI_API_KEY` | `glm-5.3-flash` |
| Z.ai (GLM) | `zai-api` | API key, international | `ZAI_API_KEY`, `ZHIPU_API_KEY` | `glm-5.3` |
| Z.ai (GLM) | `zai-cn-api` | API key, China | `GLM_API_KEY` | `glm-5.3` |
| Alibaba (Qwen) | `qwen-plan` | Coding plan, international | `ALIBABA_CODING_PLAN_API_KEY` | `qwen3.7-plus` |
| Alibaba (Qwen) | `qwen-plan-cn` | Coding plan, China | `ALIBABA_CODING_PLAN_API_KEY` | `qwen3.7-plus` |
| Alibaba (Qwen) | `dashscope` | API key, international | `DASHSCOPE_API_KEY` | `qwen3.7-plus` |
| Alibaba (Qwen) | `dashscope-cn` | API key, China | `DASHSCOPE_API_KEY` | `qwen3.7-plus` |
| Moonshot (Kimi) | `kimi-intl` | Kimi Code plan, international | `KIMI_CODING_API_KEY`, `KIMI_API_KEY` | `k3` |
| Moonshot (Kimi) | `kimi-cn` | Kimi Code plan, China | `KIMI_CODING_API_KEY`, `KIMI_API_KEY` | `k3` |
| MiniMax | `minimax-plan` | Coding plan, international | `MINIMAX_CODE_API_KEY`, `MINIMAX_API_KEY` | `MiniMax-M3.1-Flash-Preview` |
| MiniMax | `minimax-cn-plan` | Coding plan, China | `MINIMAX_CODE_CN_API_KEY`, `MINIMAX_CN_API_KEY`, `MINIMAX_API_KEY` | `MiniMax-M3.1-Flash-Preview` |
| MiniMax | `minimax-api` | API key, international | `MINIMAX_API_KEY` | `MiniMax-M3` |
| MiniMax | `minimax-cn` | API key, China | `MINIMAX_CN_API_KEY` | `MiniMax-M3` |
| Tencent | `tencent-plan` | Coding plan, China | `TENCENT_CODING_PLAN_API_KEY` | `tc-code-latest` |
| Volcengine | `volcengine-plan` | Coding plan, China | `ARK_CODING_PLAN_API_KEY` | `doubao-seed-evolving` |
| OpenCode | `go` | OpenCode Go subscription | `OPENCODE_GO_API_KEY`, `OPENCODE_API_KEY` | `glm-5.3-flash` |
| OpenCode | `opencode-zen` | Zen, pay as you go | `OPENCODE_API_KEY` | `glm-5.3-flash` |
| OpenRouter | `openrouter` | API key | `OPENROUTER_API_KEY`, `OPEN_ROUTER_API_KEY` | `openai/gpt-4o` |
| Together | `together` | API key | `TOGETHER_API_KEY` | none; pass `--model` |
| Groq | `groq` | API key | `GROQ_API_KEY` | none; pass `--model` |
| Cerebras | `cerebras` | API key | `CEREBRAS_API_KEY` | none; pass `--model` |
| Mistral | `mistral` | API key | `MISTRAL_API_KEY` | none; pass `--model` |
| Fireworks | `fireworks` | API key | `FIREWORKS_API_KEY` | none; pass `--model` |
| DeepInfra | `deepinfra` | API key | `DEEPINFRA_API_KEY`, `DEEPINFRA_TOKEN` | none; pass `--model` |
| SiliconFlow | `siliconflow-intl` | API key, international | `SILICONFLOW_API_KEY` | none; pass `--model` |
| SiliconFlow | `siliconflow-cn` | API key, China | `SILICONFLOW_CN_API_KEY` | none; pass `--model` |
| NVIDIA | `nvidia` | API key | `NVIDIA_API_KEY` | none; pass `--model` |
| Hugging Face | `huggingface` | Access token | `HF_TOKEN` | none; pass `--model` |
| Cohere | `cohere` | API key | `COHERE_API_KEY` | `north-mini-code-1-0` |
| Meta | `muse` | API key | `MODEL_API_KEY`, `META_MODEL_API_KEY`, `META_API_KEY` | `muse-spark-1.3` |
| Ollama Cloud | `ollama-cloud` | API key | `OLLAMA_API_KEY` | none; pass `--model` |
| Ollama | `ollama` | Local, no key | none | none; pass `--model` |
| LM Studio | `lmstudio` | Local, no key | none | none; pass `--model` |

`summonr zai`, `summonr alibaba`, and the other provider names run the route you last picked in `/login`; see [Choosing a route](#choosing-a-route). `perplexity` is off, because Sonar on chat completions runs no user-defined tools.

Any word after `summonr` that is not a command or an endpoint is refused with a suggestion when it looks like a mistyped command (`summonr sesions`). Use `-p` to send it as a prompt:

```sh
summonr kimi-intl -p "What is in Cargo.toml?"
```

## Sign in or use a key

1. Export the provider's key, for example `export ANTHROPIC_API_KEY=...`, or run `summonr login NAME` to store one. The prompt disables echo.
2. Run `summonr NAME`.

On an endpoint that needs no key, `summonr login NAME` says so and reports the endpoint ready, or names the missing model. A built-in name is refused by `summonr login NAME --url`. An unknown endpoint name suggests the closest name, or `summonr login NAME --url URL`.

| Command | Effect |
| --- | --- |
| `summonr login` | One line per endpoint with its state and label |
| `summonr login NAME` | Prompt for a key, or ask sign-in or key when the endpoint offers both |
| `summonr login NAME --key` | Skip the method question; storing a key signs that row out of its plan. Refused on a row that only signs in, such as `copilot` |
| `summonr login NAME --model M` | Change the model of an endpoint you added without `--url`. On a built-in endpoint, `--model` needs `--default` |
| `summonr login NAME --default` | Set `default` and `default_model`; signs in first when the endpoint is not set up, and `--model M` picks the saved model |
| `summonr login NAME --help` | Help for `login`; `logout`, `diff`, `profile`, `install`, `export`, `doctor`, `classify`, `worktrees`, and `mcp` take `--help` the same way |
| `summonr logout NAME` | Forget the key and any login token, and name the key that applies next |
| `summonr logout NAME --remove` | Delete an endpoint you added and forget its key; clears `default` and `default_model` when `default` named it |

`summonr logout NAME` says what it removed: `Signed out of NAME` for a sign-in token, `Removed the stored key for NAME` for a key, or that nothing was stored (also on an endpoint that needs no key).

An endpoint can offer both a sign-in and a key. A stored sign-in wins over any key, and `summonr logout NAME` returns the endpoint to its key. `auth.label` names the subscription in the method question.

## Coding plans

| Plan | Endpoint | How it works |
| --- | --- | --- |
| ChatGPT Plus or Pro | `openai` | Sign in with ChatGPT, next to `OPENAI_API_KEY`. Runs on the subscription's usage through the Responses API |
| SuperGrok or X Premium+ | `grok` | Device sign-in at `auth.x.ai`, next to `XAI_API_KEY` |
| Claude Pro or Max | `claude` | Runs through your own Claude Code; see below |
| GitHub Copilot | `copilot` | `summonr login copilot` is a GitHub device login; chat exchanges that token for a Copilot session token |
| zai, Qwen, MiniMax, Kimi Code, Tencent, Volcengine, OpenCode Go | see [Pick a provider](#pick-a-provider) | Coding-plan routes take the key their vendor's console issues for the plan |

`config.example.toml` carries ready endpoints for other subscriptions.

### ChatGPT

The first sign-in registers summonr with OpenAI and stores the issued client id. `~/.summonr/host-id` holds the install id that OpenAI asks for on every sign-in. A sign-in that does not grant `chatgpt.tokens.use.direct` is refused, so the endpoint never shows signed in without subscription usage.

OpenAI caps each app per week in ChatGPT settings → Usage. When the cap is reached, the error names that page after the standard retry policy.

### Claude through Claude Code

The `claude` endpoint starts `claude` for each turn and speaks the Agent SDK protocol to it. The sign-in stays in Claude Code (`claude auth login`), and summonr never reads the token.

Claude Code's own tools are off: the model calls summonr's tools through an in-process MCP server, under summonr's policy and checks. A permission request for any Claude Code built-in tool, such as `EndConversation`, is refused. Turns on one session run one at a time.

The next turn in the same summonr process resumes the Claude Code session. A new process, a switch from another endpoint, or a failed turn starts a new session that gets the earlier turns as text. A prompt typed after a cancelled tool call starts a fresh run instead of finishing the abandoned one.

Claude Code runs with your environment, so `CLAUDE_CONFIG_DIR`, proxies, and CA settings apply. summonr's system prompt reaches it through a temporary file (`--system-prompt-file`) with `--system-prompt-snapshot off`, so a changed prompt applies on resumed turns too.

`base_url` names the program: `[endpoint.claude] base_url = "/path/to/claude"` picks another install. summonr never starts Claude Code just to list models. `summonr endpoints --check` looks for the program and says when it is missing, and the `/model` picker shows the models Claude Code offered in its last session, or the Anthropic models from the catalog before the first one.

## Add your own server

Add any OpenAI-compatible server from the command line:

```sh
summonr login local --url http://127.0.0.1:8080/v1
summonr local
summonr logout local --remove
```

When the server lists exactly one model, summonr picks it; otherwise pass `--model NAME`. The same command with an existing name updates that endpoint in place.

Or add it in `~/.summonr/config.toml`:

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8080/v1"
model = "whatever-the-server-calls-it"
```

A local server needs no `api_key`. Overlay `wire = "anthropic"` on any `[endpoint.NAME]` row for an Anthropic-wire server. No classify endpoint ships: add an `[endpoint.NAME]` row with `wire = "decisions"` to use `summonr classify`. See [`config.example.toml`](../../config.example.toml).

### Local servers

When a local built-in such as `ollama` or `lmstudio` is not running, the error says so and names the preset's `start` command (`ollama serve`, `lms server start`). With no model set, the hint lists the models the server has.

Ollama gives a model only a 4k context below 24 GiB of VRAM and cuts longer prompts silently, which loses summonr's tools, and its OpenAI API cannot raise it. Start it with a larger context, the command summonr names when Ollama is not running:

```sh
OLLAMA_CONTEXT_LENGTH=65536 ollama serve
```

## Which endpoint starts

summonr starts on the first endpoint and model that applies:

1. The endpoint word and the `--model` flag.
2. `SUMMONR_ENDPOINT` and `SUMMONR_MODEL`.
3. The last endpoint and model you picked or used, kept in `~/.summonr/recent.json`. It changes when you pick a model and after a turn succeeds.
4. The default: `default` and `default_model` in `config.toml`.
5. The first endpoint that is set up.

A `default` or last-used endpoint that no longer exists or has no key prints one note, and summonr falls back to the next rule. Only an endpoint named on the command line is a hard error. The header shows which rule chose the endpoint, for example `(default)` or `(last used)`, until you switch.

In the `/model` picker, `alt+d` sets the selected row as the default, and `alt+d` on the default row clears it.

## List and check endpoints

`summonr endpoints` lists the endpoints that are set up, from files on disk and with no network call. With none set up, it lists them all.

| Flag | Effect |
| --- | --- |
| `--all` | Add the endpoints that are not set up |
| `--check` | Ask each listed endpoint for its models through `GET /models` |

A server you added that has no cached model list is asked on a plain listing too. The model count comes from the last listing in `~/.summonr/models.json` and reads `stale` after `models_listing_secs` (a day). A host that serves its model list away from its chat base sets `models_url`; `perplexity` does, with chat at `/chat/completions` and models at `/v1/models`.

Each row shows the key state and where the key comes from: `key: ANTHROPIC_API_KEY`, `key: file`, `key: keychain`, `signed in`, or `no key needed`. `--check` adds `key rejected`, `rate limited`, `offline`, or `error`. A keyless built-in with no model reads `needs model`.

Both `summonr login` and `summonr endpoints` size each column to its longest value. A row with no model shows only its URL, and endpoints that are set up come first in each group.

## Where keys live

Keys live in `~/.summonr/credentials.toml`, a plain TOML file with mode 0600 that only your account can read. A plaintext `api_key` in `config.toml` is moved into that file the first time summonr runs, and summonr says so once.

summonr resolves a key in this order:

1. The host's environment variable.
2. The store.
3. `api_key` in `config.toml`.
4. The built-in default.

An endpoint with no key fails with the missing-key message. Only local and keyless endpoints run without one.

### Keychain

To use the OS store instead, set `secrets = "keychain"` in `config.toml`, or `SUMMONR_SECRETS=keychain`.

| OS | Store |
| --- | --- |
| macOS | One Keychain entry holding every secret, read and written through `/usr/bin/security`, so the one-time "Always Allow" goes to Apple's signed tool and survives every rebuild of summonr |
| Windows | Credential Manager, one entry per secret |
| Linux | Secret Service, one entry per secret |

A locked keychain is reported, never worked around with a file. The file and keychain modes never fall back to each other.

## Providers and routes

The provider list comes from one data table in the source, `src/endpoint/presets.toml`, where each `[[preset]]` row is one route. A row summonr cannot honor fails startup by name.

### Choosing a route

`/login` lists each provider once. On a provider with routes, the detail pane shows buttons for the region and for the coding plan or the API key. Your choice is saved as `[provider.NAME] region = "cn"` and `access = "plan"` in `config.toml`. With no choice saved, summonr takes the first route whose key is set (in `config.toml`, the environment, or the store), else the first route.

`summonr NAME` takes a provider or a route name, so `summonr zai-cn` runs the China plan. No route shares its provider's name: a provider name follows the saved choice (with the plan picked, `summonr anthropic` runs Claude Code), and a route name runs that route (`summonr anthropic-api`). `/login ROUTE`, for example `/login claude` or `/login zai-cn`, saves that route as the provider's choice and opens it.

`summonr login NAME` checks and stores a key on the route that NAME runs. The last-used endpoint records the provider, so a toggle holds after a restart.

| Table | Effect |
| --- | --- |
| `[endpoint.PROVIDER]` | Overlays whichever route runs, under that route's own table |
| `[endpoint.ROUTE]` | Overlays one route |
| `[endpoint.NAME]` with its own `base_url` | Your endpoint; always wins over a provider name |

### Prompt caching and tools

Prompt caching follows each host's documented session hint, so a session's requests reach a warm cache:

| Host | Hint |
| --- | --- |
| OpenAI, xAI, Mistral, DeepSeek-style rows | Session id as `prompt_cache_key` |
| OpenRouter | `x-session-id` header |
| Fireworks | `x-session-affinity` header |
| xAI | `x-grok-conv-id` header |
| DashScope pay-as-you-go | `cache_control` breakpoints, Alibaba's explicit cache, which its implicit cache does not guarantee |

GPT-5, GPT-6, and Codex models see an `apply_patch` tool instead of `edit` and `write`, because OpenAI trains them on that patch format. It runs through the same edit engine. Every other model sees `edit` and `write`.

## Model catalog

`summonr endpoints --check` and the `/model` picker download the [models.dev](https://models.dev) catalog (`https://models.dev/api.json`, about 350 KB) for model names, context windows, reasoning and vision support, and prices. They fetch it even when every endpoint is local. `-p` runs, interface start, and a plain `summonr endpoints` do not fetch it.

The copy is cached in `~/.summonr/models-dev.json` and fetched again at most once every `models_catalog_secs` (an hour), with `If-None-Match`. Without a cached copy, summonr uses a snapshot built into the binary. The picker shows cached models at once and asks an endpoint again only when its list is missing or stale.

| Setting | Effect |
| --- | --- |
| `SUMMONR_MODELS_URL` | Fetch from another URL, such as a mirror you host |
| `models_catalog = false` in `config.toml` | Turn the fetch off |

## Provider config keys

Endpoints carry what a provider needs, so a new provider is config, not code.

| Key | Effect |
| --- | --- |
| `grant = "device_code"` | Device login |
| `grant = "authorization_code"` | Browser sign-in with PKCE (S256) |
| `token_style = "json"` | The token endpoint takes a JSON body instead of a form |
| `scheme` | Carrier for the token: `bearer` (the default for every login), `api_key`, or `google_key` |
| `pkce = true` | Add a PKCE challenge to a device login |
| `[…auth.authorize_params]`, `[…auth.token_params]` | Provider parameters on the two requests |
| `[…auth.claim_headers]` | Map a header name to a claim read from the token, so an account id travels on every request |
| `auth.label` | Name of the subscription in the sign-in or key question |
| `models_url` | Model list URL when it differs from the chat base |

The browser flow picks its return path from `redirect_uri`:

| `redirect_uri` | Behavior |
| --- | --- |
| Loopback, or unset | Opens a local port, catches the provider's redirect, and closes. Unset picks a free port; set it when the provider registered a fixed one. Port `0` keeps the provider's path and picks the port |
| Any other URL | Paste-back: summonr asks for the code the browser shows and accepts the `code#state` form. No port opens |

When the redirect carries a `client_id`, summonr stores it with the token and uses it for every refresh and the next sign-in. A provider that registers the app on first sign-in hands out its id this way.

## Reasoning effort and price

`effort = "low"` on an endpoint, `--effort LEVEL`, or `/effort` sets the reasoning effort for that endpoint. `/effort` and `tab` are remembered per endpoint. With no effort set and none remembered, summonr uses `high` when the model catalog says the model reasons, and sends no effort otherwise. [Reasoning effort](reference.md#reasoning-effort) lists the levels each model accepts.

`price = { input, output, cached, cache_write }` in USD per million tokens prices every round in the footer; `cache_write` defaults to `input`. A workflow role keeps its own endpoint's effort and price. `--effort` and `/effort` set the driving endpoint and any role whose endpoint does not set them.
