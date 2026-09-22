def is_valid(isbn):
    digits = [10 if c == "X" else int(c) for c in isbn.replace("-", "")]
    if len(digits) != 10:
        return False
    total = sum(d * i for i, d in enumerate(digits, 1))
    return total % 10 == 0
