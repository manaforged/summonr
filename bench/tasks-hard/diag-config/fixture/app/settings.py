import json
import os


def load(path, env=os.environ):
    cfg = {}
    if os.path.exists(path):
        cfg = json.loads(open(path).read())
    for k, v in env.items():
        if k.startswith("APP_"):
            cfg[k[4:].lower()] = cfg.get(k[4:].lower(), v)
    return cfg
