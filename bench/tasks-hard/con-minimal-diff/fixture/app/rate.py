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
