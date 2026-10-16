import json
from collections import defaultdict
import random
import statistics
from datetime import datetime


def seconds(span):
    if not span or not span.get("started_at") or not span.get("finished_at"):
        return None
    start = datetime.fromisoformat(span["started_at"].replace("Z", "+00:00"))
    end = datetime.fromisoformat(span["finished_at"].replace("Z", "+00:00"))
    return (end - start).total_seconds()


def interval(values, seed=20260916, samples=10000):
    if not values:
        return (None, None, None)
    rng = random.Random(seed)
    medians = sorted(
        statistics.median(rng.choice(values) for _ in values) for _ in range(samples)
    )
    return (statistics.median(values), medians[int(0.025 * samples)], medians[int(0.975 * samples) - 1])


def covered(spans):
    total, end = 0.0, None
    for start, stop in sorted(spans):
        if end is None or start > end:
            total += stop - start
            end = stop
        elif stop > end:
            total += stop - end
            end = stop
    return total


def fmt(value, digits=0):
    if value is None:
        return "–"
    return f"{value:,.{digits}f}"


def cell(values, digits=0):
    median, low, high = interval(values)
    if median is None:
        return "–"
    return f"{fmt(median, digits)} ({fmt(low, digits)}–{fmt(high, digits)})"


def hit_tokens(usage):
    if "prompt_cache_hit_tokens" in usage:
        return usage["prompt_cache_hit_tokens"]
    return (usage.get("prompt_tokens_details") or {}).get("cached_tokens") or 0


def miss_tokens(usage):
    if "prompt_cache_miss_tokens" in usage:
        return usage["prompt_cache_miss_tokens"]
    return usage.get("prompt_tokens", 0) - hit_tokens(usage)


def requests(paths):
    by_label = defaultdict(list)
    for path in paths:
        for line in open(path):
            row = json.loads(line)
            if "label" in row:
                by_label[row["label"]].append(row)
    return by_label
