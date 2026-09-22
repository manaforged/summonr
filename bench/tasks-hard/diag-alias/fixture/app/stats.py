def dedupe(items):
    seen = []
    for x in items:
        if x not in seen:
            seen.append(x)
    items[:] = seen
    return items


def average(items):
    if not items:
        return 0.0
    return sum(items) / len(items)


def unique_average(items):
    deduped = dedupe(items)
    return average(deduped)
