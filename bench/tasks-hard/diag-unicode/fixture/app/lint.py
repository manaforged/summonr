def check(lines, limit=10):
    errors = []
    for n, line in enumerate(lines, 1):
        width = len(line.encode("utf-8"))
        if width > limit:
            errors.append((n, width))
    return errors


def report(lines, limit=10):
    return [f"line {n}: {w} chars over limit" for n, w in check(lines, limit)]
