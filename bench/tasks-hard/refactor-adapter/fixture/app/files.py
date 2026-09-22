import json
import os


def save(path, key, record):
    os.makedirs(path, exist_ok=True)
    with open(os.path.join(path, key + ".json"), "w") as f:
        json.dump(record, f)


def load(path, key):
    with open(os.path.join(path, key + ".json")) as f:
        return json.load(f)
