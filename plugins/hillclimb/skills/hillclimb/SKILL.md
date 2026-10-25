---
name: hillclimb
description: Predict a performance bound, change one parameter, and keep the change only when the measurement moves toward that bound.
---

# Hillclimb

1. Name the metric, the target, and the command that prints the metric.
2. Compute the bound. For a kernel, the bound is the minimum of the peak arithmetic rate and the bytes divided by the bandwidth. If the workspace prints a phase line or a roofline, run that command and keep the line.
3. Write the prediction. The prediction is the number the bound gives for this change. A timed run that starts without that number does not count.
4. Change one parameter that the bound says can move the metric. Leave the harness frozen.
5. Run the same command. Keep the change only when the metric moves toward the prediction by more than the run-to-run range and the correctness check still passes. Otherwise revert the change.
6. Stop when the measurement sits on the bound, or when the next idea does not change the prediction. Leave the bound as it is when the measurement misses it.

Report the metric, the prediction, the measurement, and whether the change stayed.
