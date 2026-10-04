# Summonr reference

This page documents the flags, config keys, environment variables, exit
codes, slash commands, panes, policy rules, workflow seats, child
sessions, and peer mail. The
[README](../../README.md) is the short form.


## Usage

```sh
summonr grok
summonr anthropic
summonr kimi -p "What is in Cargo.toml?"
```

Summonr starts on the first seat that applies: the endpoint and
`--model` flags, then `SUMMONR_ENDPOINT` and `SUMMONR_MODEL`, then the
last seat you used, then the default (`default` and `default_model` in
`config.toml`), which applies only when there is no last seat. `alt+d` in the `/model` picker sets the selected row
as the default, and `alt+d` on the default row clears it. The header
shows which rule chose the seat, for example `(default)` or
`(last used)`, until you switch.

Add any other host in `~/.summonr/config.toml`:

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8080/v1"
model = "whatever-the-server-calls-it"
api_key = "local"
```

Then `summonr local`. `effort = "low"` on a row (or `--effort LEVEL`,
`/effort` in the interface) sets the reasoning effort for that
seat. `/effort` and `shift+tab` are remembered per seat. A seat with no
effort set, and none remembered, uses `high` when the model catalog says
the model reasons, and sends no effort otherwise. Reasoning effort below
lists the levels each model accepts. `price = { input, output, cached, cache_write }` in USD per million tokens (`cache_write` defaults to `input`)
prices every round in the footer. A workflow seat keeps
its own row's effort and price; `--effort` and `/effort` set the
driving seat and any role seat whose row does not say.
`summonr endpoints` lists the models of each name through `GET /models`. Each row shows the key state: `ready`, `needs key`, `sign in`, `key rejected`, `rate limited`, `offline`, `error`, or `no key needed`. `summonr login NAME`
prompts for an API key. `grant = "device_code"` runs a device login, and
`grant = "authorization_code"` runs a browser sign-in with PKCE (S256).

The browser flow picks its return path from `redirect_uri`:

- A loopback `redirect_uri`, or none at all, opens a local port, catches
  the provider's redirect, and closes. Omit it and Summonr picks a free
  port; set it when the provider registered a fixed one. Port `0` keeps
  the provider's path and lets Summonr pick the port.
- When the redirect carries a `client_id`, Summonr stores it with the
  token and uses it for every refresh and the next sign-in. This is how
  a provider that registers the app on first sign-in hands out its id.
- Any other `redirect_uri` switches to paste-back: Summonr asks for the
  code the browser shows and accepts the `code#state` form. No port opens.

Rows carry the rest of what a plan needs, so a new provider is config,
not code:

- `token_style = "json"` when the token endpoint takes a JSON body
  instead of a form.
- `scheme` sets the carrier for the token — `bearer` (the default for
  every login), `api_key`, or `google_key`.
- `pkce = true` adds a PKCE challenge to a device login.
- `[…auth.authorize_params]` and `[…auth.token_params]` add provider
  parameters to the two requests.
- `[…auth.claim_headers]` maps a header name to a claim read from the
  token, so an account id travels on every request.

Set `SUMMONR_NO_BROWSER=1` to print the link instead of opening it.

`config.example.toml` carries ready rows for other plans. The `claude-pro`
seat is off: Anthropic's terms do not allow a third-party app to sign in
with a Claude Pro or Max plan.

A row can offer both a sign-in and a key. `summonr login NAME` then asks
which one; `--key` skips the question. A stored sign-in wins over any
key, and `summonr logout NAME` returns the row to its key. `auth.label`
names the plan in that question.

`openai` offers Sign in with ChatGPT (Plus or Pro) next to
`OPENAI_API_KEY`, and runs on the plan's usage through the Responses
API. The first sign-in registers Summonr with OpenAI and stores the
issued client id; `~/.summonr/host-id` holds the install id that OpenAI
asks for on every sign-in. OpenAI caps each app per week in ChatGPT
settings → Usage; when the cap is reached, the error names that page
after the standard retry policy. The plan path rejects
`max_output_tokens`, sampling fields, and hosted tools; on a plan token
Summonr sends no `prompt_cache_key` and groups its tools in one
`namespace`, as that route requires. A sign-in that does not grant
`chatgpt.tokens.use.direct` is refused, so the row never shows signed in
without plan usage.

`grok` offers a SuperGrok or X Premium+ sign-in next to `XAI_API_KEY`,
through the device flow at `auth.x.ai`.

Keys live in `~/.summonr/credentials.toml`, a sealed ChaCha20-Poly1305
blob (mode 0600, written through a rename). The wrapping key is a
32-byte item in the OS store (`summoner` / `seal`). `cat` of the file
shows ciphertext. File mode also needs that keychain item: if the
keychain is locked, Summonr reports an error and keeps `config.toml`
untouched. The key prompt disables echo and stores the key;
`summonr login NAME --key` skips the method question on a row that
offers both, and storing a key signs that row out of its plan.
`summonr logout NAME` forgets the key and any login
token, and names the key that applies next. A plaintext `api_key` in
`config.toml` is moved into that file the first time Summonr runs, and
it says so once. Resolution order is the host's environment variable,
the store, peer stores, `api_key` in `config.toml`, then the built-in
default. An endpoint with no key fails with the missing-key message;
only local and keyless endpoints run without one. `--key` is refused on
a row that only signs in, such as `copilot`.

Peer stores are read live: OpenCode's
`~/.local/share/opencode/auth.json` lends `type: "api"` keys, plus
unexpired `type: "oauth"` access tokens on bearer seats; Kimi's
`~/.kimi-code/credentials/kimi-code.json` lends its unexpired token.
A peer key reports as `key · opencode` or `key · kimi-code` in
`summonr login`. A stored `summonr login NAME` key wins over a peer
key; `summonr logout NAME` returns the endpoint to the peer. Configured
endpoints whose `base_url` host matches a built-in share that
built-in's peer mapping.

If you want the OS store instead, set `secrets = "keychain"` in
`config.toml` (or `SUMMONR_SECRETS=keychain`). On macOS that is one
Keychain entry holding every secret, read and written through
`/usr/bin/security`, so the one-time "Always Allow" is granted to
Apple's signed tool and survives every rebuild of Summonr; Windows uses
Credential Manager and Linux the Secret Service, one entry per secret.
A locked keychain is reported, never worked around with a file, and
the two modes never fall back to each other.

Built-ins: chat/completions seats `zai`, `zai-cn`, `glm`, `glm-intl`,
`ollama`, `deepseek`, `kimi`, `grok`, `copilot`, `openrouter`, `together`,
`groq`, `cerebras`, `mistral`, `muse`, `zen`, `go`, `opencode`,
`fireworks`, `deepinfra`, `minimax`, `minimax-cn`; the Responses seat
`openai`; native wires `anthropic` (Messages) and `gemini`. No classify seat ships: add an
`[endpoint.NAME]` row with `wire = "decisions"` to use `summonr classify`. OpenRouter is a
chat seat (`OPENROUTER_API_KEY`); `/model` lists its catalog the same
way it lists OpenCode. `together`, `groq`, `cerebras`,
`mistral`, `fireworks`, `deepinfra`, and `ollama` carry no default model: pass `--model`. `deepseek` defaults to `deepseek-v4-flash`. `summonr login
copilot` is GitHub device login;
chat then exchanges that token for a Copilot session token. Kimi is a
key (`KIMI_CODING_API_KEY`, `KIMI_API_KEY`, or `MOONSHOT_API_KEY`) or a
token in `~/.kimi-code/credentials/kimi-code.json`. Environment keys are the host's own
(`XAI_API_KEY`, `ZAI_*`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`,
`TOGETHER_API_KEY`, `GROQ_API_KEY`, `CEREBRAS_API_KEY`,
`MISTRAL_API_KEY`, `META_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY`,
`OPENCODE_API_KEY`, `OPENCODE_GO_API_KEY`, `DEEPSEEK_API_KEY`, `FIREWORKS_API_KEY`,
`DEEPINFRA_API_KEY`, `MINIMAX_API_KEY`, `MINIMAX_CN_API_KEY`). Overlay `wire = "anthropic"`
on any `[endpoint.NAME]` row. See [`config.example.toml`](../../config.example.toml).

```sh
summonr --goal "make cargo test pass"
summonr --steps PLAN.md
```

`summonr --name TEXT` (or `/name TEXT`) names a session; `--session`
takes a name or an id prefix, `summonr sessions` lists both, and
`summonr export [ID|NAME]` prints the transcript as Markdown.
`summonr doctor --logs` lists the last 20 per-run diagnostic logs. `summonr doctor`
prints the running binary path and reports crashed or incomplete runs.
Logs older than 14 days are removed unless their process is still active.
Session directories under `~/.summonr/sessions/` with no file change for 30
days are removed once per start, except the session that run opens. Set
another period in days, 1 or more, in `~/.summonr/config.toml`:

```toml
[sessions]
retain_days = 90
```

`--goal` keeps one objective until `.summonr/check.toml` is green and a
judge says stop. The judge answers `done`, `continue`, `stalled`, or
`impossible`; the last two end the goal with the reason. A goal has no
turn cap; it runs until the judge stops it, a budget ends it, or you
cancel it. `--max-tokens` and `--max-secs` bound every mode. Every goal turn restates the objective
to the model, so a compacted context cannot lose it. `--steps` hands
the model one numbered item at a time. A turn that ends while a
background command it started is still running is deferred, with no
verdict and no repair strike, until the job exits or the model kills
it. The harness remembers that job across turns; a later turn cannot
complete while it runs. A `### Heading` under
`## Steps` groups the items that follow it into a phase. Every turn
carries the ledger to the model and to the transcript: phases as Roman
numerals, `[x]` done, `[/]` this step, `[ ]` later, `[!]` blocked.
Both edit this directory. `--worktree` isolates in a git worktree
(`<repo>-worktrees/<id>`). Exit 0 then commits the files the run changed as
`summonr: <objective>` (files over 5 MB and anything the run did not
touch are left in place and named), fast-forwards your branch, and
removes the tree; a tree that cannot fast-forward or still holds
uncommitted files is kept and named, and the exit code is 1.
`--here` is the default.

### Flags

| Flag | Effect |
| --- | --- |
| `-e`, `--endpoint NAME` | Use endpoint NAME. A bare endpoint name as the first word does the same. |
| `-p`, `--print` | Run one prompt and exit. |
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

The last of a pair (`--trust`/`--no-trust`, `--net`/`--no-net`) wins.
An option that takes a value rejects a value that starts with `-`:
`--name -x` is an error, not the name `-x`.

`summonr -p` with no prompt reads the prompt from stdin until EOF. So
does `summonr` with no prompt when stdin or stdout is not a terminal:

```sh
git diff | summonr kimi -p
```

### Config keys

Top-level keys in `~/.summonr/config.toml`:

| Key | Values | Effect |
| --- | --- | --- |
| `splash` | `true` (default), `false` | Show the startup banner. |
| `animations` | `true` (default), `false` | Animate the interface. |
| `fps` | 1 to 240, default 25 | Interface frame rate. |
| `diff_layout` | `split` (default), `unified` | Diff pane layout. |

Keys on an `[endpoint.NAME]` row:

| Key | Effect |
| --- | --- |
| `context_window` | Context size in tokens. Without it, Summonr asks the provider. |
| `idle_timeout_secs` | Seconds a response stream may stay silent before the request fails. At least 1; default 240. |
| `headers` | A table of extra HTTP headers sent on every request. |
| `fallback` | A list of endpoint names. When a request fails with an error that a retry cannot fix, the session moves to the next name. |
| `compact_tokens` | Compact the context at this token count. With `context_window` also set, the smaller value applies. |
| `keep_reasoning` | `true` sends earlier reasoning back to the model on later requests. |
| `auth.token_url` | Token endpoint of a login. Required with `auth.client_id`. |
| `auth.label` | Name of the plan shown when a row offers both a sign-in and a key. |
| `auth.required_scope` | A scope the token reply must grant, or the sign-in fails. |
| `auth.device_url` | Device authorization endpoint for `grant = "device_code"`. |

### Environment variables

| Variable | Effect |
| --- | --- |
| `SUMMONR_HOME` | Summonr home directory. Default `~/.summonr`. |
| `SUMMONR_BASE_URL` | Use this OpenAI-compatible base URL as the endpoint when no endpoint is named. |
| `SUMMONR_API_KEY` | API key for `SUMMONR_BASE_URL`. |
| `SUMMONR_MAX_TOKENS` | `max_tokens` sent with `SUMMONR_BASE_URL`. |
| `SUMMONR_KEEP_REASONING` | When set, `SUMMONR_BASE_URL` keeps reasoning as `keep_reasoning = true` does. |
| `SUMMONR_SYSTEM_PROMPT` | Replace the whole system prompt. `SYSTEM.md`, `AGENTS.md`, `CLAUDE.md`, and skills are not added. |
| `SUMMONR_MODELS_URL` | URL of the model catalog. |
| `SUMMONR_CAPTURE_DIR` | Write raw provider request and stream frames as JSON lines to this directory. |
| `SUMMONR_VERSION` | Installer: release tag to install. Default `latest`. |
| `SUMMONR_INSTALL_DIR` | Installer: install directory. |
| `SUMMONR_LIBC` | `install.sh` on Linux: `musl` (default) or `gnu`. |

### Reasoning effort

The levels are `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, and
`max`. Each model accepts some of them. Summonr sends the nearest level
the model accepts, and the lower one when two are equally near. `max` on
a model that stops at `high` sends `high`. `medium` on a model that
accepts only `low` and `high` sends `low`.

A model that takes no effort setting gets no effort field, so a level
you pick cannot fail the request. With no effort set and none
remembered, Summonr uses `high` for a model the catalog marks as
reasoning. For any other model it sends no effort field, and the model
uses its own default.

`none` turns thinking off where the model can turn it off. If the model
cannot, Summonr sends the lowest level it accepts.

Anthropic Messages (`anthropic`, or any row with
`wire = "anthropic"`), by model id:

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

Gemini (`gemini`), by model id. Levels for `gemini-2.5` models set the
thinking budget:

| Model | Levels | `none` |
| --- | --- | --- |
| `gemini-2.5-pro` | `low`, `medium`, `high` | sends the `low` budget |
| Other `gemini-2.5` models | `low`, `medium`, `high` | sets the budget to 0 |
| `gemini-3-pro` | `low`, `high` | sends `low` |
| `gemini-3` models that contain `pro`, `3.7`, or `3.8` | `low`, `medium`, `high` | sends `low` |
| Other `gemini-3` models | `minimal`, `low`, `medium`, `high` | sends `minimal` |

Other built-in seats:

| Seat | Levels | `none` |
| --- | --- | --- |
| `zai`, `zai-cn`, `glm`, `glm-intl` | `glm-5.3`: `low`, `high`, `max`. `glm-5.2`: `minimal` to `max`. Earlier models: no effort setting. | `glm-5.3`: sends `low`. `glm-5.2`: sends `none`. Earlier models: `thinking` type `disabled`. |
| `deepseek` | `minimal` to `max` | `thinking` type `disabled` |
| `kimi` | `k3`: `low`, `high`, `max`. `kimi-k2` models: no effort setting. | `k3`: sends `low`. `kimi-k2.6`: `thinking` type `disabled`. Other `kimi-k2` models: sends nothing. |
| `grok` | `grok-4.7` and `grok-4.6`: `low` to `xhigh`. Others: `low`, `medium`, `high`. | sends the lowest level |
| `muse` | `minimal` to `xhigh` | sends `minimal` |
| `mistral` | `high` | sends `none` |
| `cerebras`, `deepinfra`, `ollama` | `low`, `medium`, `high` | sends `none` |
| `openrouter` | `minimal` to `xhigh`, sent as `reasoning` with an `effort` key | sends `none` |
| `together`, `groq`, `copilot`, `zen`, `go`, `opencode`, `fireworks` | `low`, `medium`, `high` | sends nothing |
| `minimax`, `minimax-cn` | no effort setting | `thinking` type `disabled` |

Summonr matches the model id against these model rows on every seat
except `copilot`, `openrouter`, and `ollama`, which pass any model
through and use the seat's own levels. A model that matches no row uses
the seat's levels. A row with no built-in host uses `low`, `medium`, and
`high`, and sends nothing for `none`.

Set the levels or the `none` style on a row to replace the built-in
behavior. A `[endpoint.NAME.models.ID]` row replaces it for one model.

```toml
[endpoint.local]
base_url = "http://127.0.0.1:8080/v1"
model = "qwen3"
reasoning_levels = ["low", "high", "max"]
thinking_off = "disabled"

[endpoint.local.models.qwen3-small]
reasoning_levels = []
```

`reasoning_levels` lists the levels the seat accepts, from `minimal` to
`max`. An empty list sends no effort. `thinking_off` sets what `none`
sends:

| `thinking_off` | `none` sends |
| --- | --- |
| `omit` | nothing |
| `none` | the level `none` |
| `disabled` | `thinking` with type `disabled` |
| `between_tools` | `thinking` with type `between_tools` |
| `chat_template_kwargs` | `chat_template_kwargs` with `enable_thinking` set to `false` |
| `unsupported` | the lowest level in `reasoning_levels` |

The `body` table of a row is merged last, so it can still set any field
by hand.

### Modes

| Mode | Enter | Leave | Job |
| --- | --- | --- | --- |
| Auto | default, `/auto` | `/plan`, `/goal` | Follow the request this turn. |
| Plan | `/plan` when `plan_mode = true`, `--plan` | `/plan accept`, `/plan reject`, `/auto` | Read-only. Reply is saved as `.summonr/plan.md`. |
| Work | `/plan accept` only | job ends (auto), `/auto` | Execute the accepted plan, then `.summonr/check.toml`. |
| Goal | `/goal TEXT`, `--goal` | judge `done` / stop, `/auto` | Loop on an objective with no written plan until checks pass and the judge says `done`. |

Work is plan after accept. Without a selected plan, a request for work
stays in plan mode. `/plan reject` discards the plan and returns to auto.

Plan mode is off by default. Turn it on with the `plan mode` row in
`/settings` or `plan_mode = true` in `~/.summonr/config.toml`. While it is
off, `/plan` is refused. `--plan` still works.
Leaving plan mode removes the sandbox that plan mode turned on; a sandbox
from `--sandbox` or `[sandbox] on` stays.

### Confirmation

Everything runs without a prompt except irreversible destruction: wiping
the system, home, or project root, `mkfs`, `dd` to a device, a fork bomb,
`chmod -R 777 /`, `terraform destroy`, cluster deletes, and dropped database
tables. Those ask first with `[ allow ]` / `[ deny ]` (or `y` / `n`). `-p` has
no one to ask, so they do not run there. Each turn's checkpoint makes the rest,
such as `git reset --hard`, recoverable with `/undo`.

`sudo` asks for your password in a dialog. The password goes to `sudo`
through `SUDO_ASKPASS`; the model never sees it, and command output is
scrubbed of it.

### Workflows

A workflow seats each step on its own model. Roles are `scout`, `plan`,
`implement`, `review`, `judge`, and `summary`; a role the workflow does
not name runs on the seat you started with.

```toml
# ~/.summonr/config.toml
workflow = "cheap-scout"      # optional default

[workflows.cheap-scout]
plan = "grok"
review = "zai"
judge = "grok"
```

`summonr -w cheap-scout`, `/workflow cheap-scout` in the interface, or the
`workflow` key picks one; `summonr workflows` lists them. In the
interface every card action has a verb too:

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

`summonr --gui` opens the session in a desktop window (a build with
`--features gui`). The window is built on Sigil: a left rail
that switches the Files, Changes, Worktrees, and Plan panes and lists this
workspace's sessions; a main column with the transcript, a composer card,
and an Auto/Plan switch; a right rail that shows the open pane with real
widgets: clickable file rows, +/− chips that open a file's hunks, an
Edit/Save file view. The title bar's changes badge is live. The same
`App`, keys, and layouts drive the terminal and the window.

The screen is a set of panes on one session. `ctrl+e` (or `/pane
files`) opens the repository tree beside the chat; `ctrl+l` the plan
ledger; `ctrl+p` hides or restores the pane. In the tree: `ctrl+→`
focuses it, `↑↓` move, `←→` collapse or expand, `PgUp/PgDn/Home/End`
jump, Enter opens a directory or attaches `@path` to your prompt, and
a right-click (or the mouse) opens a menu: attach, open in `$EDITOR`,
insert path. `ctrl+←` or `esc` returns to the composer. `/pane off`
closes the pane. `/pane focus`, `run`, and `review` are
presets, `/pane "chat | files 30%"` sets the split, and
ctrl+shift+←/→ resizes it. The layout persists in `~/.summonr/config.toml`
as `layout`. `ctrl+o` opens the settings modal, where every hotkey is
editable; the choice is stored under `[hotkeys]` in the same file.
`ctrl+n` folds every thinking block, then every tool output (`tools =
"hidden"`), then shows them all again; a click still opens one block.

`ctrl+x` (or `/pane diff`, `/diff`) shows the changes against the base:
the worktree base when you passed `--worktree`, `HEAD` otherwise. Each
row is `+added −removed path`; Enter or `→` opens that file's hunks —
one card per `@@` block with a `pending`, `accepted`, or `rejected`
badge. Inside a file `↑↓` picks a hunk, Enter toggles `accepted`, `r`
reverts the hunk (`git apply --reverse` of just that block), `n` opens
a note field (`ctrl+enter` saves, `esc` cancels) that rides to the model
with your next prompt, and `←` or `esc` returns to the list. The
right-click menu offers the same actions. Verdicts and notes persist in
`.summonr/review.json`. `/diff PATH` jumps to one file. After a turn that
changed the tree, the transcript gets one `changes: N files · +a −r · vs BASE ·
/pane diff` line. The same data is `summonr diff [PATH]` (the patch), `summonr diff
--stat`, and `summonr diff --json`.

`→` on a file in the tree (or `/view PATH`, or the menu's view file)
reads it in the view pane with line numbers; lines the diff added are
green with a `+`. `↑↓`, `PgUp/PgDn`, `Home/End` scroll, `o` opens it in
`$EDITOR`, `esc` returns to the tree. `e` edits the file in place: the
keyboard goes to the file (type, Enter, arrows, Home/End), the caret is
the inverted cell, `ctrl+s` saves, and `esc` leaves. With unsaved text a
second `esc` discards it. The tree, the diff, and the view
follow the file system: a change on disk relists, re-diffs, or reloads
the pane within about half a second.

In the diff pane `t` keeps only the files the last turn edited (the
header says `· last turn`), `c` copies the patch on show to the clipboard
(OSC 52 in the terminal; the system clipboard in the window), and a pane
80 columns or wider shows a file's hunks side by side, old on the left.
`summonr diff --turn N` (or `--turn last`) is the same filter for the
latest session in this directory.

While a turn runs, a row above the composer shows a spinner, what the
model is doing (`thinking`, or the tool it is running), the round, the
seconds so far, the output rate, and the queue count. A message sent during
the turn waits in the queue for the next turn; `enter` on an empty composer
sends the queue into the running turn instead.

The output rate is tokens per second. While text streams it is an
estimate: characters received in the last three seconds, divided by the
characters per token measured on the previous round (4 until then), over
that span. When the provider reports the round's usage, the rate becomes
exact: output tokens over the time between the first and last streamed
chunk, or over the whole round when nothing streamed, as in a round that
only called tools.

When a turn ends while the terminal is not focused, summonr rings the bell
and sends a desktop notification (OSC 9) naming the workspace. `notify` in
`config.toml`, or the Notify row in `/settings`, sets `off`, `turn` (the
default), or `all`, which also notifies when an approval is waiting.

The footer shows the workspace and branch on the left and, on the right, the
session's spend when the model has a price or the provider reports a cost,
then the context and cache meters.

When a command asks for a password through `SSH_ASKPASS` or `SUDO_ASKPASS`,
such as an ssh key passphrase, a **Credential required** dialog opens.
Type the password and press `enter` to send it, or press `esc` to cancel.
Closing the dialog also cancels the request. The password goes to the
waiting command over a private socket, never to the model, and later
command output shows it as `[secret]`.

### Keys

| Key | Idle composer | During a turn |
| --- | --- | --- |
| `enter` | send | queue for the next turn; on an empty composer, send the queue into the running turn after the current tool calls |
| `tab` | complete a `/command`, its argument, `@path`, a `/server:prompt`, or an `@server:uri` | the same |
| `↑` | recall earlier prompts; with input queued, pull it back into the editor | |
| `esc` | dismiss; twice within two seconds opens the session tree | cancel the turn |
| `ctrl+c` | clear the draft; on an empty draft, twice within two seconds quits | cancel the turn |
| `ctrl+d` | quit when the draft is empty; it only quits | |
| `pgup` `pgdn` | scroll the transcript a page | same |
| `home` `end` | on an empty draft, jump to the top or the latest reply | same |
| `alt+d` | in the sessions picker, twice deletes the selected session; in `/login`, signs out; in `/model`, sets the default | |
| `alt+k` | in `/login`, pastes a key on a row that also offers a sign-in | |
| `alt+p` | in `/model`, adds or removes the selected model from favorites | |
| `alt+o` | in `/model`, shows or hides models that are older than six months or have a newer model in the same family | |
| `F5` | retry a failed turn; in `/model`, refresh the providers | |
| `ctrl+j` | newline (default; editable in options) | |
| `ctrl+g` | edit the draft in `$EDITOR` (default) | |
| `shift+tab` | cycle reasoning effort: none, minimal, low, medium, high, xhigh, max | takes effect when the turn ends |
| `alt+←` `alt+→` `alt+b` `alt+f` | word left or right | |
| `ctrl+w` `alt+backspace` | delete the word before the caret | |
| `alt+d` | delete the word after the caret | |
| `ctrl+u` `ctrl+k` | delete to the line start or end | |

### Commands

`/help` lists these by job. Old names such as `/rollback`, `/tree`, and
`/net` still work for one release and print the new name once.

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
| `/exit` | quit summonr |
| `/model [NAME] [LEVEL]` | choose the model; LEVEL sets the effort |
| `/effort [LEVEL]` | set the reasoning effort; no LEVEL opens a slider |
| `/workflow [NAME]` | choose models for plan, work, and review |
| `/login [NAME]` | sign in to a provider or store its key. Rows sit under `ready`, `needs attention`, and `not set up`; at 96 columns or more a detail pane shows the row's key, sign-in, models, and actions |
| `/plan` | enter plan mode: read and plan, no edits |
| `/plan accept` | run the saved plan |
| `/plan reject` | leave plan mode without running the plan |
| `/auto` | return to auto mode |
| `/goal TEXT` | work toward an objective across turns |
| `/steps PLAN.md\|clear` | run a plan's `## Steps` one at a time |
| `/diff [PATH]` | changes against the base, or one PATH |
| `/view PATH` | read a file in a pane |
| `/pane [NAME]` | files, ledger, diff, worktrees, jobs, peers, or a layout |
| `/settings` | theme, tool output, reasoning, modules, keys |
| `/permissions [net\|write]` | sandbox network and write paths |
| `/help` | list commands and keys |

Typing `/` opens the command list above the composer. Each row shows
the command, its argument, and one line of help. After `/model `,
or `/login `, the list shows the values that command takes:
the current endpoint's cached models or the endpoint names. `↑` and `↓` move, `tab` completes, `enter` runs, and `esc`
hides the list.

A draft that starts with `!` runs in the command sandbox before any
model round: `!cmd` shows the output as a tool card and then sends
`$ cmd` plus the output as the prompt; `!!cmd` shows the output only.

An image pasted from the clipboard, or attached with `@shot.png`, shows in
the transcript, and so does any image the model shows you. `read` on an
image path, or a markdown image link such as `![plot](out/plot.png)` in a
reply, shows that file. In Ghostty, Kitty,
WezTerm, and Konsole the picture renders through the Kitty graphics
protocol; in iTerm2 and Warp through inline images; in other terminals as
half-block cells. `SIGIL_GRAPHICS=kitty`, `iterm2`, or `none` overrides the
detection, and tmux turns it off. A picture is 48 rows tall at most, and a
picture scrolled partly off the screen shows the rows that are on it.
`read` on an image reports its type and size, and the picture shows in
the transcript; the model attaches it as `@path` to look at it. `/view
shot.png`, or `→` on an image in the tree, shows the picture in the view
pane.

A fenced `mermaid` block in a reply renders as a diagram in the
transcript: flowcharts and state diagrams with box-drawing edges, sequence
diagrams with lifelines and frames, pie charts as bars, and `xychart` /
`xychart-beta` as labeled bars. A fenced `chart` block with `label value`
rows draws the same bars. Other Mermaid kinds show their source in a
frame. Flow, sequence, and pie drawings come from Sigil's `mermaid`
widget, so they look the same in every terminal.

A paste of eight or more lines, or a thousand or more bytes, folds into
`[pasted N lines #k]` in the draft. Summonr expands every token back to
the pasted text when the prompt is sent or queued; `ctrl+c` discards
them with the draft.

While a turn runs, the composer status also counts waiting input
(`2 queued, ↑ edits`). The terminal title reads
`summonr · <workspace> · working` or `· ready`, and the terminal bell
rings once when a turn ends while the terminal is not focused.

`/settings` switches tool output between compact and full, sets chain
of thought to `collapsed` (the default: one dim line with a line
count), `expanded`, or `off`, and sets turn receipts to `off` (the
default), `collapsed`, or `expanded`.

`/export` writes the session as markdown to `~/.summonr/exports/<id>.md`;
`/export PATH` writes it where you say. It works while a turn runs.
`ctrl+b` (the default copy hotkey) copies the latest assistant reply. `/copy all` copies the full
transcript. Both use OSC 52 in the terminal and the system clipboard in the
window. `ctrl+t` releases the mouse to the terminal so you can select and copy
any transcript span; press it again to restore mouse interaction.

`/pane worktrees` lists the `summonr/*` worktrees of this repository:
branch, commits ahead of the base, dirty or clean, the session that ran
there, and the directory. Enter or `→` opens that worktree's diff (`esc`
comes back); the right-click menu offers show diff, merge (the same
ff-only merge a run ends with; a dirty tree is refused), remove
worktree (only when clean and fully merged), and insert path. `summonr
worktrees` prints the same rows; `summonr worktrees --json` as JSON.

`/pane jobs` lists this workspace's background commands — the records
under `.summonr/jobs/`. Each row is the command and its state
(`running`, `exit N`, `killed`, or `dead`); Enter or `→` expands the
job's status lines, and the right-click menu offers view log (the
job's `.log` in the view pane) and kill job (the same stop
`run_command` performs for `{"job": ID, "kill": true}`). The list
refreshes on its own while the pane is open.

`/pane peers` lists the other live sessions on this machine — the mail
roster, minus this one. Each row is `@name`, model, workspace folder,
and the peer's status (`idle`, `working`). Enter or a right-click
mention inserts `@name` into the prompt; a prompt that names a peer is
delivered to its inbox instead of running a turn.

The card takes the keyboard (↑↓ role, enter picks a seat, `d` returns
it to the driving seat, ←→ cycles saved workflows, `s` saves) and the
mouse: click a role to pick its seat, drag a role onto another to copy
the seat, click a saved name to apply it. The settings modal, model
picker, and authentication card take clicks as well. The session log
records the model behind every reply.

With a `scout` seat, a `--steps`, `--goal`, or plan-mode job opens with
a read-only brief of the files, conventions, and helpers the objective
touches. With a `review` seat, a turn whose checks passed is read by
the reviewer before it completes; a rejection sends the findings back
as the next instruction and counts as a failed check.
## Child sessions

The `task` tool delegates work to fresh child sessions. Only an interactive
session offers it: `-p`, `--steps`, `--goal`, and `summonr acp` runs have no
steering channel, so the model does not get `task` there. A model can emit
several `task` calls in one response. Summonr runs them concurrently.
Each `implement` child edits the parent workspace. Implement children get
the parent's file, shell, and language-server tools; background commands
stay with the parent. The parent verifies after they finish. Pass
`--worktree` on the session to isolate the whole run.

A `research` child reads the parent workspace. It cannot write files or
start background commands. Its shell commands run in a read-only sandbox
(Seatbelt on macOS, bubblewrap on Linux) that summonr turns on for it, so
it can run checks, searches, and scripts but cannot change the workspace;
where no sandbox is available it has no shell.
Each child uses the endpoint context window. If the parent has a token or
time budget, the children share what remains of it; otherwise a child runs
until it finishes or you cancel it. Children have no web tools or another
`task` tool. Every child tool call, result, and answer streams into the
parent transcript under the child id. A research child that finished
before you pressed Esc keeps its result: the model reads it on the next
turn instead of an `aborted` answer.

`task` with `background: true` starts the child and returns its id at
once. `task` with `child: ID` reports its state, elapsed time, and last
action while it runs, and its full result once it finishes; `kill: true`
stops it. A turn does not end while a background child runs: Summonr
waits, then defers the turn the way it does for a background command,
until the child finishes or the model kills it.

## Tools

The model receives the full advertised schema every round
(`Loop::advertised_tools`). Core names live in `src/tools/catalog.rs`.

Reads return `N:hash|text`. A stale or ambiguous tag fails before write.
`read` lists a directory, shows an image, or searches (`pattern` / `glob`
or `include`, with `literal` and `caseSensitive`). Search hits are
`path:N:col:line`; `col` is the 1-based UTF-16 column of the match, which
`lsp` takes as `character`. With `tags` true, hits are `path:N:hash|line`,
and `N:hash` is the tag that `edit` takes. `edit` covers substring replace (`old`/`new`), line
replace (`tag`/`new`), insert (`tag`/`text`), delete (`op: delete`), a
batch (`edits`), and a rename or move (`path`
with `to`). `write` creates or overwrites one file, or several (`files`); over an
existing file it returns `wrote PATH (N lines, replaced M lines)`, not the
diff. `read` pages by 50 KB by default; an explicit `limit` is capped at
1000 lines. `run_command` takes `workdir`; its output keeps about 50 KB of
head and tail, a single line up to 8 KB, and the full log path is named
only when that cut something. Only exact `cat FILE`, single-file `head`,
and `sed -n A,Bp FILE` without a `workdir` are answered by `read`; `ls`,
`wc`, `tree`, `grep`, and `rg` run in the shell. A foreground command waits
up to two minutes; past that it keeps running as a background job and returns
its id and the call that waits for it, unless `timeout_secs` was set, which
stops it instead. Where a command cannot become a job (a child session, Plan
mode, a restricted write policy, or the jobs module turned off), it is stopped
after an hour. A job poll waits until the job exits or the wait limit
passes.

| Name | When | Notes |
| --- | --- | --- |
| `read` | always | File, directory, image, or search (`literal`, `caseSensitive`, `include`) |
| `edit` | not plan | Change or move a file; hashes from `read`; `edits` for a batch |
| `write` | not plan | New or empty files; `files` for several |
| `run_command` | always | Sandbox; `workdir` / `background` / `job` / `kill`; full log path |
| `todo` | `/steps`, `/goal`, `--steps`, `--goal` | Replaces the Work todo list; status `pending`, `active`, or `done` |
| `skill` | skills catalog exists | Loads one `SKILL.md` by name |
| `lsp` | `lsp` on | Diagnostics for a `path`; definition, `references`, or `new_name` at `line` and `character` |
| `web` | `search` on (off by default) | `query` searches; `url` fetches as markdown, text, or html |
| `task` | `task` module, interactive session | Child session |
| `agents` | `mail` module | List live agent sessions by name |
| `send` | `mail` module | `to` (a name, an id, or `all`) and `text`: write to a peer's inbox |
| `inbox` | `mail` module | Read mail that arrived since the last read |
| `board` | `mail` module | `action` get, set, or list; `key`, `value` |
| `mcp` | an MCP server has over 8 KB of tool schemas | Lists those servers' tool names; `query` returns matching tools with schemas; `tool` and `arguments` call one |

The old names (`read_file`, `list_dir`, `grep`, `show_image`,
`write_file`, `replace_text`, `replace_lines`, `insert_after`,
`delete_lines`, `replace_texts`, `edit_file`, `diagnostics`,
`definition`, `web_search`, and `web_fetch`) still execute and are not
advertised. Plan mode advertises
`read` and `run_command`, plus `skill` and, with `search` on, `web`. It does not advertise
`edit`, `write`, or `todo`.

## Modules

Every optional capability is a **module** compiled into the binary: `search`,
`lsp`, `jobs`, `task`, `skills`, `mcp`, `mail`, and `gui`. That list is the
built-in module set: curated, switchable, and inside the harness process RSS. `search` and `mail`
are off by default so unused tool schemas stay out of the first `prefix_hash`; `mcp` is on, and adds schemas only for servers you configure;
the mail roster and `@` messages work without the `mail` module. `summonr modules` lists them with their state; the Modules rows in `/settings`
in the interface switches one and writes `[modules]` in `~/.summonr/config.toml`.
After the first `prefix_hash` in this process, that write is for the next
process only. A workspace `.summonr/config.toml` may turn a module off for that
project but cannot turn one on. A new compiled tool is a row on
[`module-api.md`](internals/module-api.md) in the same change; if a skill or MCP can
do the job, it is not a module. Summonr runs no third-party code inside the
harness; modules read declarative files under `.summonr` where they need user
input.

`read` with `pattern` or `glob` honors `.gitignore`, and lists matching files
when the pattern is empty.
With the `search` module on, `web` with `query` reads Bing without a key, and `url` fetches one page.
Both go through Leyline with a browser identity, so no search API key or
self-hosted search service is needed. `[search]` takes `count`,
`allowed_domains`, and `excluded_domains`. Language modules cover Rust,
TypeScript/JavaScript, Python, and Go. They select the server from the file and
a project marker, then keep one connection per module in a polyglot checkout.
Rust passes initialization options that disable check-on-save, build scripts,
proc macros, and `cargo --all-targets`. Explicit Cargo commands still run
through `run_command`.

A workspace `.summonr/config.toml` may force a built-in module:

```toml
[lsp]
module = "rust"
```

Repository config cannot supply executable argv. A trusted
`~/.summonr/config.toml` may use `[lsp] command = ["server", "--stdio"]` when a
built-in module does not fit.
Language servers run as trusted host processes with the inherited environment,
outside the command sandbox. They can access files and network resources with
your account's permissions. Rename edits returned to Summonr still pass through
its file policy. Disable the `lsp` module when you do not trust the server or
the project configuration it reads.

Unknown flags exit 1. A red verify gate exits 2 (`-p`, or a bounded
goal that ends red). A run that stops short exits 3: a budget breach, a
stalled or impossible goal, a bounded goal the judge did not accept,
or an aborted turn. On Unix, `SIGINT` exits 130, `SIGTERM` 143, and
`SIGHUP` 129; on Windows, Ctrl+C exits 130. A signal exit skips the
`--worktree` finish, so the worktree and its branch stay in place.
JSON turn receipts keep model text separate from the changed-path list
and every configured completion check. `verified` on a receipt is true
only when at least one check ran and every check passed; a turn with no
configured check completes with `verified: false`.

## Mail

Agent sessions on this machine share a mailbox: files under
`~/.summonr/mail` — a roster, one inbox per session, and one board per
workspace. There is no server; each session appends and polls files. Mail is
peer text, never user instructions, and each session's own permissions still
apply.

Every summonr session joins the roster under a name: the one you set with
`--name` or `/name`, or else the workspace folder and a number, such as
`summonr-1`. Child sessions stay off the roster, and `SUMMONR_MAIL=off` keeps
a session off it. `/agents` lists the sessions on the roster.

To message a session, type `@` in the composer and pick it from the list, or
type its name, such as `@summonr-2 the schema landed`. A prompt that names a
live session goes to that session's inbox instead of to the model. The mail
arrives there as `[mail from @summonr-1 …]`; `[agent] mail` in settings
decides whether it starts a turn or waits for the next one. Reply the same
way, with `@summonr-1`.

The `mail` module, off by default, also gives the model the tools `agents`,
`send`, `inbox`, and `board`. `send` takes a name, an id, or `all`.

The same mailbox is a CLI, so any agent CLI — Claude Code, Codex, a script —
can take part without a server:

```
summonr mail                  # who is on this machine
summonr mail send <name|id|all> <text>
summonr mail inbox <id> [--clear]
summonr mail board [get <key>|set <key> <text>]
```

A foreign agent takes part by picking an id: it reads
`summonr mail inbox <id>`, and anyone can write to it with
`summonr mail send <id> <text>`. Its messages arrive from `cli` unless it
writes its own inbox name into replies. `inbox <id> --clear` prints the mail
and empties that inbox. It refuses the inbox of a live summonr session,
because that session keeps its own read position. A lock file next to each
inbox makes a send, a read, and a clear wait for each other, so no message
is lost between them.

## Limits

Everything in this section describes a contained session. Without
`--sandbox`, or `on = true` under `[sandbox]`, none of it applies: the shell
is uncontained and the command and file policy is off.

Each flag alone gives these modes:

| Mode | File tools | Shell commands | Shell network |
| --- | --- | --- | --- |
| Default | Full access with your account's permissions | Uncontained; command policy bypassed | Your account's access |
| `--sandbox` | Reads any path the sandbox can read; writes under the workspace and extra writable roots | Contained; workspace, extra writable roots, and temporary storage writable | Allowed, or denied with `--no-net` |
| `--plan` | Reads; edit and write denied | Contained; shell writes and background jobs denied | Denied |

With containment on, `read` accepts an absolute path outside the workspace,
which is what the shell can already read. `edit` and `write` stay inside the
writable roots:
the launch directory, `[sandbox] write` in `~/.summonr/config.toml`, and
`--add-dir`. For work that spans sibling repositories, put the parent on
that list, launch Summonr in the parent directory, or pass `--add-dir`.

Commands use Seatbelt on macOS and bubblewrap on Linux. Windows AppContainer
is unfinished and needs a build with `--features windows-sandbox`; Git for Windows and user-installed toolchains stay blocked.
A missing backend is a hard stop. Sandboxed sockets are allowed by default.
`--no-net` or `/permissions net off` denies them. That flag
does not change file permissions. Model requests and language servers use
host networking separately from shell commands.

`/permissions net on` allows sockets for the rest of the session,
`/permissions net once` allows only the next command, and
`/permissions net off` denies again.
The interface also prompts when a sandboxed command fails with a network error:
`[ allow ]` turns the session network on and retries the command; `[ deny ]`
returns the failure to the model. `/permissions` alone reports the network and write state.

Auto mode grants a write outside the writable roots for this session and
retries the tool. It does not write `~/.summonr/config.toml`. Plan mode still
refuses writes. `/permissions write PATH` persists to that list without a prompt.
`/permissions write --session PATH` grants for this session only.
`/permissions write` alone lists
the extra roots.
Writers such as `cp`, `ln`, `mv`, `mkdir`, `touch`, and `install` may then
target those paths, and the OS sandbox allows the write. `write` and `edit`
accept files under them too. `~` and `$HOME` expand before the check.
The same irreversible commands ask before they run, with or without the
sandbox. Under `--strict` they never run, and neither do force pushes, `sudo`,
pipes into a shell, history rewrites, or writes to protected paths. Under `--sandbox` or `--strict`, matches on your own `[policy] deny`
list are refused without a prompt.
In the sandbox, a write to a protected path outside the workspace
(`.git/hooks`, `.git/config`, `~/.ssh`, `~/.gnupg`, `~/.summonr`, shell
startup files) is allowed for the session without a prompt, and a note in
the transcript names the path. A protected file inside the workspace (`.env`)
is denied. Reads of `.env` and `~/.ssh` / `~/.gnupg` are blocked too.

A sandboxed `cargo`, `rustup`, `npm`, `npx`, `pnpm`, `yarn`, `bun`, or `go`
command that fails with `Operation not permitted` offers the matching home
directory (`~/.cargo`, `~/.npm`, or `GOPATH` / `~/go`) as the grant.

Containment is opt-in: `--sandbox`, or `on = true` under `[sandbox]` in
`~/.summonr/config.toml`. An uncontained session writes anywhere your account can and
bypasses command and file policy. `--no-sandbox` is accepted and changes nothing.
`--plan` contains commands, still denies `edit` and `write`, and refuses shell writes
because read-only inspection requires a sandbox.
`--strict` reapplies policy restrictions but does not restore process containment.

A build tool that writes outside the writable roots fails inside the sandbox
with `Operation not permitted`. Name those directories in
`~/.summonr/config.toml` (or pass `--add-dir`) and every contained command,
including a research child's, can write there:

```toml
[sandbox]
write = ["~/.cargo/registry", "~/code"]
```

A `~/` prefix expands to your home directory. A listed path that does not
exist is skipped at startup; it is still a writable root for policy checks.

A provider reply of 429, a 5xx status, or a connection error is retried up
to three times with a wait that doubles from 1 s to 8 s. A `Retry-After`
header replaces that wait; one over 60 s stops the retries so the turn can
fall back to the next endpoint. A reply that already streamed output is
retried once, then the turn falls back.

A language server gets 30 s to start and 30 s to answer an `lsp` call. The
diagnostics check after an edit waits 5 s and is skipped when the server is
slower. A server that misses a deadline keeps running, so a slow first index
can finish in the background.

Summonr does not steer the model. Tool results reach it unchanged, and it
decides when a turn is done; only a token or wall-clock budget that you set
ends a turn early. A reply cut off at the output limit gets one note saying
so. The compaction summary request carries no tools, and a failed summary
retries after 8 rounds.

User policy is `~/.summonr/policy.toml`; repository policy is `.summonr/policy.toml`.
A nonempty command allow list must match on every overlay (intersection).
Deny lists accumulate. `write` globs in policy.toml are a narrowing filter
on paths relative to a writable root; extra roots themselves come only from
`[sandbox] write`, `--add-dir`, and `/write`. Empty lists add no restriction,
except that `strict = true` with an empty allow list denies all commands.
Repository policy can narrow user permissions, never widen them. Children
inherit these restrictions.

`confirm` picks which commands ask first. `confirm = "destructive"`, the
default, asks only before irreversible destruction (see Confirmation).
`confirm = "risky"` also asks before `sudo`, force pushes, pipes into a shell,
git commands that discard work or rewrite history, and writes to protected
paths. The stricter value wins across overlays.

```toml
confirm = "risky"
```
In plan mode `edit` and `write` are denied. Shell commands stay read-only, so
source edits and builds are refused.
A restricted write list makes contained shell commands read-only and disables
background commands; file tools still use the list.

- Home is still `~/.summonr`. Env is still `SUMMONR_*`.
- An older `mage` binary may still be on `PATH`. Use `summonr`.

## Extend

The built-in modules are the binary (modules above). What you add lives in files and other
processes so the harness RSS and the frozen prefix stay small. Other products
name that package a plugin; Summonr does not load guest code.
[Extending summonr](extending.md) is the task page.

| Layer | Where | What the model sees |
| --- | --- | --- |
| Skill | `~/.summonr/skills/<name>/SKILL.md` or `.summonr/skills/<name>/SKILL.md` | One `skill` tool. The body is a later tool result. |
| MCP | `[mcp.NAME]`; approved `.mcp.json`; `import_mcp = true` for Claude/Codex configs | `server__tool` aliases that connected before the first `prefix_hash`; a server over 8 KB of schemas is reached through one `mcp` tool. `/server:prompt` and `@server:uri` expand server prompts and resources into the user message. |
| Hook | `[hooks]` `pre_tool`, `post_tool`, `compact`, `stop` (a command or a list) | Not a tool. `pre_tool` may block a call (stdin is JSON). |
| Plugin | `summonr install PATH` (Claude Code layout) | Its skills, MCP tools (`<plugin>-<server>__tool`), hooks, and `bin/` |
| CLI | `run_command` | A binary on `PATH` or under `~/.summonr/bin/<name>/`. Policy, danger prompt, and `--sandbox` apply. Copying files must not execute the binary. Workspace `.summonr/bin` is not put on `PATH` unless the user approved it the same way as `.mcp.json`. |

`summonr install PATH` copies a plugin directory to `~/.summonr/plugins/<name>/`. Copy does not execute. Restart so new MCP aliases join the next `prefix_hash`. `summonr install --list` prints installed plugins. There is no marketplace and no in-process loader.

After the first `prefix_hash`, advertised tools and skill catalog names stay frozen until process exit. A module switch in `/settings` persists for the next process and does not rebuild this session. Hub reconnects that would change aliases are ignored. Plan vs work may still drop `edit`/`write`, and a steps or goal run adds `todo`.

When the workspace is trusted, summonr injects `~/.agents/AGENTS.md`, `~/.claude/CLAUDE.md`, and `~/.config/devin/AGENTS.md` as user context, then `AGENTS.md` and `CLAUDE.md` at the workspace root as project context. A file that resolves to one already loaded is skipped, and a section repeated across files is sent once.

`SYSTEM.md` replaces the built-in preamble: `~/.summonr/SYSTEM.md` always applies, and `<workspace>/SYSTEM.md` applies only when the workspace is trusted. If either file is present and nonempty, the stock preamble is dropped in favor of it.

Memory: `~/.summonr/skills/memory/SKILL.md` should tell the model to read and write labeled notes under a directory named in that file (default `~/.summonr/skills/memory/notes/`). Do not put notes in the system paragraph.

Intent permissions: a `pre_tool` hook that exits nonzero blocks the call. Keep argv policy in `policy.toml`; do not add a second classifier unless it is a compiled module row.

Remote TUI: `summonr acp`, or `ssh` and the terminal. There is no HTTP control plane.

Browser: `web` fetches when the `search` module is on. A Playwright MCP is a child process, not Chromium inside summonr.

`summonr export` is Markdown of `~/.summonr/sessions/<id>/session.jsonl`. It does not import transcripts from other tools.

Search stays `read` (`pattern` / `glob`) or `run_command` (`rg`). A 1k-token impact graph is a named MCP server, not a second grep tool.

## Theme

`theme` in `config.toml` selects `space` (the default), `nord`, `dawn`
(a low-glare light theme), or `auto`, which picks `space` or `dawn` from
the terminal's background. The Theme row in `/settings` cycles them.
`background = "terminal"` leaves the background unpainted so the
terminal's own background (and any transparency) shows through;
`"theme"`, the default, paints the theme's `base` color. The Background
row in `/settings` switches between them. Every other color is a named
token you can override in a theme file:

```toml
accent = "#698cd2"
background = "#0c0c14"
code_bg = "#1e202a"
```

Copy [`theme.example.toml`](../../theme.example.toml) to `~/.summonr/theme.toml`, or to
`<workspace>/.summonr/theme.toml` for one project. Unset tokens keep the preset.
Values are `#rgb`, `#rrggbb`, or `#rrggbbaa`; malformed values are ignored.
Both files apply on top of the chosen theme. The tokens are
`base`/`background`, `text`/`foreground`, `gray`, `dim`, `thought`, `accent`,
`green`, `red`, `error`, `yellow`, `cyan`, `black`, `user_bg`, `tool_bg`,
`code_bg`, `diff_add_bg`, `diff_del_bg`, `code_comment`,
`code_string`, and `code_keyword`.

## Herdr

When `summonr` runs inside a [Herdr](https://herdr.dev) pane, it reports
lifecycle state through Herdr's CLI (`pane report-agent`). `idle` is the
prompt; `working` is a turn. The Summonr session id is the native session
reference. `/name TEXT` becomes the pane title and `$summary` token.
Child sessions do not report, so a nested `task` does not steal the pane.
Herdr has no built-in `summonr` kind. This reporter is the integration.

## Docs

- [Work](architecture.md)
- [`config.example.toml`](../../config.example.toml)
- [`policy.example.toml`](../../policy.example.toml)
