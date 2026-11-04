# Native workers and SQLite audit analysis

The plugin adds instructions for coordinating native Summonr workers and an optional Python CLI for importing their audit records into SQLite. It does not run workers, track their lifecycle, allocate candidate files, or deliver messages.

Summonr's `task`, `send`, budgets, cancellation, and journals remain authoritative. The domain's existing evaluator and file-ownership tools remain authoritative for candidates and promotion.

## Install and use

```sh
summonr plugin install plugins/lanes
```

The `/lanes:lanes` command loads the coordinator instructions. Start each worker with one native `task` call. There is no claim/bind/renew/report sequence and no separate run specification.

Plugin binaries are available to `run_command`. From an ordinary shell, invoke the installed plugin's `bin/lanes` by path. The reference CLI requires Python 3.11 or later.

## Import audit records on demand

```sh
lanes --db .summonr/audit.sqlite import --session SESSION_ID
lanes --db .summonr/audit.sqlite audit --session SESSION_ID
```

Import creates the audit database when needed. It invokes the existing `summonr session export SESSION_ID --json` command, never an agent run. The native exporter must provide schema-1 JSONL records and its final record-count manifest. A running session is exported as a consistent prefix, not declared complete.

For an archived export or a pipeline:

```sh
lanes --db .summonr/audit.sqlite import --file session.jsonl
summonr session export SESSION_ID --json | lanes --db .summonr/audit.sqlite import
```

Raw provider fields require enabling the existing capture facility before the model call:

```sh
SUMMONR_CAPTURE_DIR=.summonr/capture summonr
lanes --db .summonr/audit.sqlite import --session SESSION_ID --captures .summonr/capture
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

Database schemas 1 and 2 upgrade to the audit-only schema without changing existing event bodies or hashes. Obsolete coordination tables, if present in an old database, are preserved as historical data; no command reads or updates them to control workers. A new database has only audit tables and views.

## Scope

This is same-host audit analysis, not a remote scheduler, sandbox, tamper-proof archive, or large-agent capacity claim. Native session journals and mail remain the source records. Database creation uses owner-only file permissions on Unix. Audit data can contain sensitive prompts and outputs; do not publish it by default.

The byte limit counts stored payloads, not indexes or filesystem overhead. Keep disk headroom and use a consistent SQLite backup for live databases. No retention process deletes evidence automatically.
