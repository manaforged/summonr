from rate import RateLimiter


def test_allows_up_to_limit():
    r = RateLimiter(limit=2, window=10)
    assert r.allow(0)
    assert r.allow(1)
    assert not r.allow(2)


def test_window_expires():
    r = RateLimiter(limit=1, window=5)
    assert r.allow(0)
    assert not r.allow(3)
    assert r.allow(6), "hit at t=0 must have expired by t=6"
