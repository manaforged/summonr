_cache = {}


def get(key, store):
    if key not in _cache:
        _cache[key] = store.get(key)
    return _cache[key]


def set_value(key, value, store):
    store[key] = value
