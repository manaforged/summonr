import math
import statistics


def log_ratios(mine, theirs, tasks, key):
    ratios = {}
    for task in tasks:
        a = [row[key] for row in mine[task] if row[key]]
        b = [row[key] for row in theirs[task] if row[key]]
        if a and b:
            ratios[task] = math.log(statistics.mean(a) / statistics.mean(b))
    return ratios


def ci(values, transform, draws):
    if not values:
        return "–"
    point = transform(statistics.mean(values.values()))
    means = sorted(
        transform(statistics.mean(values[t] for t in draw if t in values))
        for draw in draws
        if any(t in values for t in draw)
    )
    return f"{point:.2f} ({means[int(0.025 * len(means))]:.2f}–{means[int(0.975 * len(means)) - 1]:.2f})"
