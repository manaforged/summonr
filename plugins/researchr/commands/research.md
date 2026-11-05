---
description: Climb one measured goal with one long-lived agent per runner, stacking judged wins.
argument-hint: "<spec.toml, or a goal to write one for>"
---

Research climb for: $ARGUMENTS

If that is a goal and not a spec file, write `.summonr/research/spec.toml` in the shape `researchr --help` and the researchr skill give, then show it to the user. Run `researchr climb SPEC` with background true; it runs until `.summonr/research/STOP` exists. Each runner session is a Summonr session named `<lane>-<k>-s<n>`. Report from `researchr status`; do not edit runner files or judge anything yourself.
