DEFAULTS = {"usr": "guest", "retries": 3}

def load(overrides=None):
    cfg = dict(DEFAULTS)
    if overrides:
        cfg.update(overrides)
    return cfg
