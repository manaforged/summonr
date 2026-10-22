# Schedules and chat

This page shows you how to run summonr on a schedule, from a chat app, from a web request, or from a GitHub issue comment.

## Run on a schedule

`summonr cron` runs a prompt or a goal on a schedule, headless, in the directory you add it from:

```sh
summonr cron add nightly --cron "0 3 * * 1-5" --goal "make cargo test pass"
summonr cron add digest --every 6h -p "summarize what changed in git log since yesterday" --mail summonr-1
summonr cron add once --at 09:00 -p "check the release notes"
summonr cron add triage --manual -p "triage the alert in the payload"
summonr cron install
```

| Schedule | Form |
| --- | --- |
| `--cron "FIELDS"` | Five cron fields: minute, hour, day, month, weekday. Takes `*`, lists, ranges, steps, and `jan`-`dec` and `sun`-`sat` names; `0` and `7` are Sunday. When both day and weekday are set, either one matches. |
| `--cron @NAME` | `@hourly`, `@daily`, `@weekly`, `@monthly`, or `@yearly` |
| `--every N` | A count with `m`, `h`, or `d`. `--every 1d` fires at midnight and `--every 2h` on even hours. |
| `--at TIME` | One time: `HH:MM` for the next such time, `YYYY-MM-DD HH:MM`, or RFC 3339. A time already past is refused. The job pauses itself after its scheduled run; running it by hand or from the webhook does not use it up. |
| `--manual` | Runs only through `summonr cron run` or the webhook |

Times are local, or in `--tz ZONE`. A time that daylight saving skips does not fire that day, and a repeated time fires once.

`summonr cron add NAME` takes one schedule, one task, and these options. `summonr cron edit NAME` takes the same flags.

| Option | Default |
| --- | --- |
| `-p PROMPT`, `--prompt PROMPT`, or `--goal GOAL` | Required: the task |
| `--dir D` | The directory you run `cron add` from |
| `-e E`, `--endpoint E` | The endpoint summonr picks when the job runs |
| `--model M` | The endpoint's model |
| `--tz ZONE` | Local time |
| `--max-secs N` | `cron_budget_secs` (1800) |
| `--max-tokens N` | No token limit |
| `--worktree` | Off: the run works in the directory |
| `--chat` | Off |
| `--notify never\|failure\|always` | `failure` |
| `--mail SESSION` | None |

### Let the OS start jobs

`summonr cron install` asks the OS to run `summonr cron tick` once a minute. No summonr process stays running. `uninstall` removes it, and `status` says whether it is installed and when the next job fires.

| OS | Runner |
| --- | --- |
| macOS | A launch agent, which runs while you are logged in. When launchd refuses one (no one signed in), summonr installs a crontab line; cron needs Full Disk Access for jobs that touch Desktop, Documents, or Downloads. |
| Linux | A systemd user timer, which stops at logout unless lingering is on (`loginctl enable-linger`); install says so. It falls back to a crontab line when systemd has no user session. |
| Windows | A scheduled task |

On macOS and Linux, install records your `HOME`, `PATH`, and `SUMMONR_HOME`, so jobs load the same user instruction files and tools as the shell you installed from. On Windows, set them as user environment variables. Store keys with `summonr login` rather than in your shell, because cron runs do not see your shell's variables.

### How a job runs

Each due job starts as its own process with `--json` and a time budget: `--max-secs`, or `cron_budget_secs` (30 minutes). `--endpoint`, `--model`, `--max-tokens`, and `--worktree` pass through. The run is a job under `.summonr/jobs` in the job's directory.

A `--goal` job is done only when the workspace check passes. A goal cannot start with `-`. A goal runs until the judge agrees it is met; a precise goal ("make `cargo test` pass") ends sooner than an open one. See [Goals and checks](goals.md).

| Situation | Result |
| --- | --- |
| The job is still running when its next time comes | The time is skipped, and the skip is logged. |
| The machine was asleep | The latest missed time runs once. Times older than `cron_catchup_secs` (7 days) are logged as expired. |
| An `--at` time passed while the machine was off past the catch-up window | Logged as expired, and the job pauses. |
| `cron_fail_pause` failures in a row (3) | The job pauses itself. `summonr cron resume NAME` restarts it. |
| The run cannot start, for example because its directory is gone | Counts as a failure. |

### Get the result

| Option | Effect |
| --- | --- |
| `--notify failure` | A desktop notification when a run fails (the default) |
| `--notify always` | A desktop notification after every run |
| `--notify never` | No notification |
| `--mail SESSION` | Also sends the result to that session's inbox |
| `--chat` | Also sends the result to every chat paired with `summonr gateway` |

`summonr cron log NAME` lists past runs with their outcome, then the last run's final answer and verdict ("check passed", "check failed", or why it stopped) and the path of its full log. `--raw` shows the raw events.

### Manage jobs

`summonr cron run NAME` runs a job now; `--notify` there overrides the job's setting for that run. Jobs live in `~/.summonr/cron/jobs.toml`. `cron add` and `cron show` name the endpoint a run uses: the one set with `-e`, or the one summonr would pick now, with the reason. `cron add` warns when that endpoint has no stored key, when `--mail` names no live session, and when `--chat` has no paired chat.

The model manages jobs with the same commands through `run_command`. Inside a cron run it cannot add, edit, remove, pause, resume, run, install, or uninstall jobs, or print a token.

## Gateway

`summonr gateway` lets a chat app or a web request start work on your machine. Nothing listens on a public port.

### Telegram

1. Open [@BotFather](https://t.me/BotFather) in Telegram and send `/newbot`.
2. Pick a display name and a username ending in `bot`.
3. Copy the token it replies with. It looks like `123456789:AA...`.
4. Store the token and start the bridge:

   ```sh
   summonr gateway setup telegram     # paste the bot token; it is stored, never shown
   summonr gateway telegram           # run the bridge here; Ctrl+C stops it
   ```

5. Message the bot. It answers with a pairing code, and the terminal shows who asked.
6. On the machine, run `summonr gateway pair CODE` to allow your Telegram account.

Pair your account before you leave the machine, because pairing needs a command on the machine. Codes expire after `gateway_pair_secs` (an hour). Until you pair them, other people who message the bot get only a pairing code they cannot use. The bridge answers only private chats, never groups or channels.

The bridge runs while its terminal runs. To keep it going after you close the terminal, start it in `tmux` or `screen`, or as a launch agent or systemd user service that runs `summonr gateway telegram --dir PROJECT`. The bridge asks Telegram for new messages, so it works behind a router or firewall.

#### Chat with a session

Each chat is one summonr session in the directory where the bridge runs. `--dir` picks another directory, and `--endpoint` and `--model` pick the model. Every message is one turn, and the answer comes back to the chat. Messages sent while a turn runs wait for the next turn, and the bot says how many are ahead. A reply says when the check failed or the run broke.

| Chat command | Action |
| --- | --- |
| `/new` | Start a fresh session |
| `/status` | Show the current session |
| `/stop` | End the running turn and its commands, then kill what is left after `gateway_stop_secs` |
| `/start`, `/help` | Show these commands |

The bridge remembers where it stopped reading, so a restart never runs a message twice. A second bridge on the same bot is refused.

A chat turn runs like `-p` with your account: it can run any command your policy allows and read or write anything you can, unless you contain it. Anything that needs an approval is denied. Pair only accounts you trust with that directory, start the bridge in a project folder rather than your home, or run it in a container.

#### Manage paired accounts

`summonr gateway status` lists paired accounts with their Telegram user ids. `summonr gateway unpair USER_ID` removes one, with its chat and any messages it queued. The token is kept where summonr keeps API keys (`~/.summonr/credentials.toml`, or the keychain with `secrets = "keychain"`); `summonr gateway setup telegram` again replaces it. The `gateway_*` settings are limits: override them under `[limits]` in `~/.summonr/config.toml`.

If you lose the phone or a paired account is compromised:

1. Stop the bridge: Ctrl+C, or stop its tmux session or service.
2. Run `summonr gateway unpair USER_ID`.
3. Revoke the token in @BotFather with `/revoke`.
4. Run `summonr gateway setup telegram` with the new token.

The old token stops working at once.

### Webhook

A webhook starts a cron job from a web request. A job you only fire from outside is a `--manual` job.

```sh
TOKEN=$(summonr cron token nightly) # the token on stdout, how to use it on stderr
summonr gateway webhook            # listens on 127.0.0.1:8765
curl -X POST -H "Authorization: Bearer $TOKEN" --data @event.json http://127.0.0.1:8765/run/nightly
```

`POST /run/NAME` with the job's token starts that job now and answers `202` with `{"job", "slot", "log"}`. `slot` is the run's start time in Unix seconds, the time `summonr cron log NAME` shows in its first column. `log` is that command, which shows how it went.

| Status | Cause |
| --- | --- |
| `401` | A wrong token or an unknown job, answered before any body is read |
| `405` | A method other than `POST` |
| `408` | The request did not finish within `gateway_read_secs` |
| `409` | The job is paused |
| `411` | A chunked body |
| `413` | A body over `gateway_body_bytes` (64 KiB) |
| `429` | A second webhook fire within `gateway_fire_secs` (60 seconds); scheduled runs and `cron run` do not count. Carries `Retry-After`. |

Every error has a JSON body `{"error": ...}`. A query string is ignored. Up to 32 requests are served at once, and a client that has not sent its headers within `gateway_head_secs` (2 seconds) is dropped, so slow clients cannot hold up real events. The listener prints one line per request (method, path, status, and job) to stderr, never the token or body. Ctrl+C, `SIGTERM`, or `SIGHUP` stops it.

The listener binds loopback only. To take events from another machine, put a tunnel or reverse proxy in front of it. `summonr cron token NAME` again rotates the token, and `summonr cron remove NAME` forgets it.

The body is saved in the job's directory under `.summonr/jobs`, and the run is told it is untrusted event data, not instructions. The run has your account's access like any other run, so serve webhooks from a container when anyone you don't trust can send the body.

Saved bodies are deleted with the run history that refers to them. What the run read from the body stays in that run's log under `.summonr/jobs` and in its session under `~/.summonr/sessions` until those are pruned (`[sessions] retain_days`), so do not send secrets you would not keep in a log.

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
      - name: Work on the request
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
          BODY: ${{ github.event.comment.body }}
        run: |
          task=$(printf '%s' "$BODY" | perl -0pe 's/<!--.*?-->//gs; s/^\/summonr //')
          ~/.local/bin/summonr anthropic --goal "$task" --json --max-secs 1800 > "$RUNNER_TEMP/summonr.jsonl"
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

The repository needs a `.summonr/check.toml` (for example `check = "cargo test"`). Without one, `--goal` stops with exit 3 and no pull request opens. When the check fails, it exits 2.

The comment reaches the workflow through an environment variable, never pasted into the script, and hidden HTML comments are stripped before the model sees it. Only people with write or admin access to the repository can start it; the first step asks GitHub for the commenter's permission.

The checkout keeps no GitHub token, so the run holds only the model key; the pull request step signs in after the run ends. Comments on pull requests are ignored, and each run uses its own branch.
