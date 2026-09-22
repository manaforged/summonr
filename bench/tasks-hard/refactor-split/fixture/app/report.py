def parse_rows(text):
    rows = []
    for line in text.strip().split("\n"):
        k, v = line.split("=")
        rows.append((k.strip(), int(v)))
    return rows


def total(rows):
    return sum(v for _, v in rows)


def render_table(rows):
    out = []
    for k, v in rows:
        out.append(f"{k:<10} {v:>5}")
    out.append(f"{'TOTAL':<10} {total(rows):>5}")
    return "\n".join(out)
