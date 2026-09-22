def parse(text):
    out = {}
    for line in text.splitlines():
        if "=" in line:
            key, _, val = line.partition("=")
            if "=" not in val:
                out[key.strip()] = val.strip()
    return out
