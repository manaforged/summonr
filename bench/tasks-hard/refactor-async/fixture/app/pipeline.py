RESULTS = []


def process(items):
    for it in items:
        RESULTS.append(transform(it))
    return RESULTS


def transform(it):
    return it.strip().upper()
