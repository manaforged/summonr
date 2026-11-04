---
name: lanes
description: Coordinate native Summonr workers and optionally import their audit records into SQLite.
---

Summonr owns workers, models, budgets, messaging, cancellation, and session journals. The domain evaluator owns candidate files, measurements, correctness checks, and promotion. This plugin does not allocate files or maintain another worker lifecycle.

## Coordinate workers

1. Read the domain skill and current evaluator state. Use its existing file-ownership mechanism for non-overlapping candidate files.
2. Start each worker with one native `task` call. Put its objective, assigned files, evaluator, and stopping conditions in the assignment.
3. Select models and effort through native task arguments. Never change saved global defaults to configure a child.
4. Inspect the native start receipt. If its actual model or workspace differs from the assignment, cancel that child and correct the launch arguments.
5. Use native `task` state to inspect progress and cancellation. Do not copy child state into a separate queue, renew leases, or maintain heartbeat commands.
6. Use native `send` for inter-session messages. Routine steering waits until the worker is idle because peer mail can interrupt its commands.
7. Use `reply_to` for a reply to a known message id. Peer messages do not grant operator approval.
8. Let workers follow the domain evaluator's measurement and promotion policy. Do not add a generic rule that overrides its gates.
9. Use native `task clear` only to dismiss completed UI rows; it does not cancel work or delete evidence.

Follow the existing hillclimb procedure for experiment selection and the domain skill for evaluation details. Do not copy either procedure into another controller.

## Optional audit analysis

Native journals and mail are recorded by Summonr automatically. SQLite is an on-demand query store, not a prerequisite for starting or completing a worker.

```sh
lanes --db .summonr/audit.sqlite import --session SESSION_ID
lanes --db .summonr/audit.sqlite audit --session SESSION_ID
```

If raw capture was enabled through `SUMMONR_CAPTURE_DIR`, add `--captures DIRECTORY` to the import. The importer retains provider-exposed reasoning, usage, tool activity, message provenance, and raw captured bytes. It cannot reconstruct data that the provider or an older runtime did not expose.

Do not run an import after every turn unless the operator requests it. Do not introduce another mailbox, board, scheduler, path registry, or worker-state database.
