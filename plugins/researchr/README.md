# researchr: research mode for Summonr

researchr climbs one measured goal by stacking judged wins. Each runner keeps one agent session working in its lane for as many edit and measure iterations as the session allows, from one fixed prompt. When the agent has a measured win it runs `researchr submit "<mechanism>"`: the harness runs the spec's `judge` on the file, records one attempt row, and on a keep writes one line to the lane's inbox and to the inbox of every lane in `ports`. Agents read their inbox after each measurement, bring their file onto a moved best with the `seed` command, and port other lanes' ideas by hand. When a session ends, the runner starts the next one, which resumes from the runner's status file and the lane's judged log. Each session is a named Summonr session, `<lane>-<k>-s<n>`, and its audit records land in the same SQLite database. The judge alone keeps or rejects; agents never report their own numbers.

Requires Python 3.11 or later.

## Run

```sh
summonr plugin install plugins/researchr
researchr climb spec.toml          # or /researchr:research spec.toml in Summonr
researchr status
touch .summonr/research/STOP       # every runner stops after its current session
```

```toml
goal = "one sentence with the metric and its direction"
prompt = "path/to/role.md"          # optional: the agent's fixed rules; default rules otherwise
agent = ["summonr", "-p", "--name", "{session}"]   # default; any harness with a one-prompt mode
log_lines = 30
session_secs = 14400

[target]
seed = "cp best/{lane}.py {file}"   # optional: runs before a runner's first session; listed as the rebase command
judge = "..."                       # required: exit 0 keeps the file as the new best; may print "gain [low, high]"; the last one printed is recorded
measure = "..."                     # any other command is listed in the brief

[[lane]]
name = "a"
runners = 2
file = "src/{runner}.py"
ports = ["b"]                       # optional: lanes whose inbox also gets this lane's keeps
```

Templates take `{file}`, `{runner}`, `{lane}`, `{session}`, `{status}` and every string field of the lane. `judge` also takes `{mechanism}`, the agent's one line, shell-quoted, and `{snapshot}`, a copy of the file taken at submit. The judge owns every keep rule (noise margin, knob-sized changes, repeats) and moving the best forward. Two sessions in a row that end within 60 s stop their runner, with an error on stderr.

| Command | What it does |
| --- | --- |
| `researchr climb SPEC` | runs every lane's runners until STOP or `--sessions` per runner |
| `researchr submit MECHANISM` | inside a runner's session: judge the file, record the verdict, write keeps to the inboxes |
| `researchr status` | per lane kept and rejected counts, and the latest submissions |
| `researchr import`, `audit` | import native session exports, query the audit |

## Import audit records on demand

```sh
researchr import --session SESSION_ID
researchr audit --session SESSION_ID
```

Import creates the audit database when needed. It invokes the existing `summonr session export SESSION_ID --json` command, never an agent run. The native exporter must provide schema-1 JSONL records and its final record-count manifest. A running session is exported as a consistent prefix, not declared complete.

For an archived export or a pipeline:

```sh
researchr import --file session.jsonl
summonr session export SESSION_ID --json | researchr import
```

Raw provider fields require enabling the existing capture facility before the model call:

```sh
SUMMONR_CAPTURE_DIR=.summonr/capture summonr
researchr import --session SESSION_ID --captures .summonr/capture
```

Captures use session and exchange ids, timestamps, and base64 bytes. They retain fields the normalized adapter does not interpret. Incomplete provider exchanges are reported as partial, not completed responses. Without capture, the importer reports that raw provider fields are unavailable. It does not reconstruct missing reasoning or past transport data.

## Query the audit log

The `event` table retains complete audit envelopes and their SHA-256 values. The `message` and `response` views expose message ids, sender/recipient ids, reply relationships, actor, delivery stage, text, model, reasoning, and usage where available.

```sql
select sender, recipient, reply_to, actor, stage, text from message;
select session, model, reasoning, usage from response;
```

Send attempt, delivery, dequeue, and administrative inbox clear are separate events. None proves that a model acted on a message. Legacy records may lack ids or actor metadata.

Exact reimports are deduplicated. Changed content at an existing source sequence is rejected. Gaps, corrupt records, and missing export manifests fail visibly. A failed import can leave earlier committed batches; replay the complete source to finish safely. Use `--allow-partial` only when explicitly archiving an incomplete capture.

`audit --full` returns bodies within an output-size limit. Larger bodies remain in SQLite. `--max-bytes` and `--record-bytes` configure archive limits; they are not model or execution budgets. Exceeding an import limit rolls back its batch without deleting earlier evidence.

Database schemas 1 to 3 upgrade to schema 4 without changing existing event bodies or hashes. Schema 4 adds the run tables `run`, `worker`, `finding` and `dispatch`.

## Scope

This is same-host audit analysis, not a remote scheduler, sandbox, tamper-proof archive, or large-agent capacity claim. Native session journals and mail remain the source records. Database creation uses owner-only file permissions on Unix. Audit data can contain sensitive prompts and outputs; do not publish it by default.

The byte limit counts stored payloads, not indexes or filesystem overhead. Keep disk headroom and use a consistent SQLite backup for live databases. No retention process deletes evidence automatically.
