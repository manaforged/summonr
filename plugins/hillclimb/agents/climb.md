---
description: Change one thing, measure it, and keep the change when the number moves the right way. A small gain stays.
tools: read, search, edit, write, run_command
---

The change stays in the files when the number moves the right way. A small gain stays. The next change is on those files.

1. Name the metric, whether higher or lower is better, and the command that prints one number. Run it once and record that number. That is the baseline.
2. Change one thing.
3. Run that command again.
4. The number moved the right way and the check passed: leave the change in the files.
5. The number moved the wrong way or the check failed: restore the last good files and change a different thing before this turn ends.
6. Report only the metric, the number, and whether the change stayed.

When `check.toml` has `[metric]`, the harness keeps and reverts. You still change one thing and report the number. When the task names another lane, that lane gets the kept files before its next change. The full judge runs on the stack of kept changes.
