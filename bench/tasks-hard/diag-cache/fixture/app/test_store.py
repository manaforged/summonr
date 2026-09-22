from store import get, set_value


def test_read_through_cache():
    store = {"a": 1}
    assert get("a", store) == 1
    assert get("a", store) == 1


def test_config_roundtrip():
    store = {"mode": "fast"}
    assert get("mode", store) == "fast"
    set_value("mode", "safe", store)
    assert get("mode", store) == "safe", "read after write must see the new value"
