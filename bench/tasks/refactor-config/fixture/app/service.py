from config import load

def whoami(overrides=None):
    return load(overrides)["usr"]
