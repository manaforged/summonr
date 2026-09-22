#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && [ "$(diff -u <(cat <<'ORIG'
class RateLimiter:
    def __init__(self, limit, window):
        self.limit = limit
        self.window = window
        self.hits = []

    def allow(self, now):
        self.hits = [t for t in self.hits if now - t < self.window]
        if len(self.hits) <= self.limit:
            self.hits.append(now)
            return True
        return False
ORIG
) rate.py | grep -c '^+[^+]')" -le 6 ]
