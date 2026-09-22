def total_cents(prices):
    cents = 0.0
    for p in prices:
        cents += p * 100
    return cents
