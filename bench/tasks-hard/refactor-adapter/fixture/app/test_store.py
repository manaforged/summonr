import pytest


def _impls(tmp_path):
    from store import FileStore, MemoryStore
    return [FileStore(str(tmp_path)), MemoryStore()]


@pytest.mark.parametrize("n", [0, 1])
def test_roundtrip(tmp_path, n):
    store = _impls(tmp_path)[n]
    store.save("k", {"a": 1})
    assert store.load("k") == {"a": 1}


@pytest.mark.parametrize("n", [0, 1])
def test_missing_key(tmp_path, n):
    store = _impls(tmp_path)[n]
    with pytest.raises(KeyError):
        store.load("nope")
