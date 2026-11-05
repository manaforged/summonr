---
name: researchr
description: Research climb for Summonr. Stack judged wins on one measured goal with one long-lived agent per runner, wins ported between lanes through inboxes, and every submission and its audit in SQLite.
---

`researchr climb SPEC` is the whole loop. Per runner: `seed` puts the current best into the runner's file, and one agent session loops edit, measure and `researchr submit "<mechanism>"` from a fixed prompt. The spec's `judge` keeps (exit 0) or rejects, one row goes into the `attempt` table, and a keep goes to the lane's inbox and its `ports` lanes' inboxes. When a session ends, the next one resumes from the runner's status file. The agent never judges itself.

## Spec

```toml
goal = "one sentence with the metric and its direction"
prompt = "path/to/role.md"
agent = ["summonr", "-p", "--name", "{session}"]

[target]
seed = "..."      # the current best into {file}
judge = "..."     # exit 0 keeps; owns noise, knob and repeat rules and moves the best; may use {mechanism}
measure = "..."   # other commands are listed in the brief

[[lane]]
name = "a"
runners = 1
file = "src/{runner}.py"
ports = ["b"]
```

The head only writes the spec, starts `researchr climb SPEC` in the background, and reports `researchr status`. To stop, create `.summonr/research/STOP`. To change runners or lanes, stop, edit the spec and start again; attempt numbers continue.

## Audit

Each attempt's session is imported after its verdict. Query with `researchr audit --session NAME` or the database: table `attempt`, views `message` and `response`, table `event`. The audit stores what the provider returned and never reconstructs reasoning a provider did not expose.
