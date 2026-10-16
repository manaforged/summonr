import math
import random
import statistics
from collections import defaultdict

def log_ratios(mine, theirs, tasks, key, keep=lambda row: True):
    ratios = {}
    for task in tasks:
        a = [row[key] for row in mine[task] if row[key] and keep(row)]
        b = [row[key] for row in theirs[task] if row[key] and keep(row)]
        if a and b:
            ratios[task] = math.log(statistics.mean(a) / statistics.mean(b))
    return ratios


def bounds(values, transform, draws, level=0.95):
    point = transform(statistics.mean(values.values()))
    means = sorted(
        transform(statistics.mean(values[t] for t in draw if t in values))
        for draw in draws
        if any(t in values for t in draw)
    )
    return point, means[int((1 - level) / 2 * len(means))], means[int((1 + level) / 2 * len(means)) - 1]


def ci(values, transform, draws, level=0.95):
    if not values:
        return "–"
    return "{:.2f} ({:.2f}–{:.2f})".format(*bounds(values, transform, draws, level))


def order(rows, subject=None):
    names = sorted({row["harness"] for row in rows})
    if subject in names:
        names.remove(subject)
        names.insert(0, subject)
    return names


def ok(row):
    return row["reward"] == 1.0 and not row["void"]


def failed(row):
    return not ok(row)


def per_task(rows, harness):
    tasks = defaultdict(list)
    for row in rows:
        if row["harness"] == harness:
            tasks[row["task"]].append(row)
    return tasks


def solved_rate(group):
    return statistics.mean(map(ok, group))


PAIRED = [
    ("input tokens per solved try", "prompt", ok),
    ("input tokens", "prompt", None),
    ("uncached input", "miss", None),
    ("output tokens", "output", None),
    ("cost", "cost", None),
    ("time", "agent_seconds", None),
    ("first-request tokens", "first_prompt", None),
    ("mean harness RSS", "harness_avg_mb", None),
    ("peak harness RSS", "harness_mb", None),
]


def draws_over(tasks, rng=None, samples=10000):
    rng = rng or random.Random(20260916)
    return [[rng.choice(tasks) for _ in tasks] for _ in range(samples)]


def task_rate(groups, solved):
    return ci({task: float(solved(list(map(ok, group)))) for task, group in groups.items()}, lambda v: v, draws_over(sorted(groups)))


def paired_line(other, mine, theirs, rng, samples):
    tasks = sorted(set(mine) & set(theirs))
    if not tasks:
        return None
    draws = draws_over(tasks, rng, samples)
    solved = {task: solved_rate(mine[task]) - solved_rate(theirs[task]) for task in tasks}
    cells = [ci(log_ratios(mine, theirs, tasks, key, keep or (lambda row: True)), math.exp, draws) for _, key, keep in PAIRED]
    return f"| {other} | {len(tasks)} | {ci(solved, lambda v: v, draws)} | " + " | ".join(cells) + " |"


def paired(rows, subject, seed=20260916, samples=10000):
    mine = per_task(rows, subject)
    lines = [
        f"Ratios are {subject} / other, geometric mean over tasks both ran (per solved try: tasks both solved at least once), 95% bootstrap over tasks. Below 1 means {subject} used less.",
        "",
        "| Other | Tasks | Solved difference | " + " | ".join(name for name, _, _ in PAIRED) + " |",
        "| --- | --- | --- | " + " | ".join("---" for _ in PAIRED) + " |",
    ]
    rng = random.Random(seed)
    for other in order(rows, subject):
        line = None if other == subject else paired_line(other, mine, per_task(rows, other), rng, samples)
        if line:
            lines.append(line)
    return "\n".join(lines)


PRIMARY = ("input tokens per solved try", "mean harness RSS")


def control(rows, subject, twin, seed=20260916, samples=10000):
    mine, theirs = per_task(rows, subject), per_task(rows, twin)
    tasks = sorted(set(mine) & set(theirs))
    draws = draws_over(tasks, random.Random(seed), samples)
    checks = [("solved difference", {task: solved_rate(mine[task]) - solved_rate(theirs[task]) for task in tasks}, lambda v: v, 0.0)]
    checks += [(name, log_ratios(mine, theirs, tasks, key, keep or (lambda row: True)), math.exp, 1.0) for name, key, keep in PAIRED if name in PRIMARY]
    lines = [f"{subject} against {twin} on {len(tasks)} tasks, 99% bootstrap over tasks. The control holds when every interval contains the no-difference value.", ""]
    held = True
    for name, values, transform, null in checks:
        if not values:
            lines.append(f"- {name}: no data")
            held = False
            continue
        point, low, high = bounds(values, transform, draws, 0.99)
        held = held and low <= null <= high
        lines.append(f"- {name}: {point:.3f} ({low:.3f}–{high:.3f}), width {high - low:.3f}")
    return "\n".join(lines + ["", f"A/A control {'holds' if held else 'fails'}."])
