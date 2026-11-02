---
description: Lead named hillclimb lanes. One child per lane, a steer message, and a scheduled tick.
argument-hint: "[lane names]"
---

Lead $ARGUMENTS.

You do not edit a lane file. Do not start a `summonr` process. Do not use the `monitor` tool.

| Job | Tool |
|---|---|
| Lane agent | `climb` |
| Start lane | `task`, agent `climb`, `background` true, `name` the lane name, `effort` when the task names one |
| Steer lane | `send` |
| Tick | `summonr cron add --every <interval> --mail "$SUMMONR_SESSION_ID" --prompt "<tick>"` |

Start every lane in one turn. The tick prompt names the lanes. On each tick, list them, then:

1. A lane with no measurement running gets its next wall by `send`. Name the wall. The lane picks the change.
2. A kept change goes to every lane that shares that work. Send the file, the change, and the number.

The child reports the metric, the number, and whether the change stayed. The full check runs on the stack of kept changes.
