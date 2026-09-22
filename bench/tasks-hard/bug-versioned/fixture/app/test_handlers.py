import pytest
from handlers_v1 import handle as h1
from handlers_v2 import handle as h2


def test_v1_shape():
    assert h1({"user": "u", "ts": 5})["ts"] == 5


def test_v2_new_shape():
    out = h2({"user": "u", "ts": 5, "meta": {"k": 1}})
    assert out["meta"] == {"k": 1}


def test_v2_accepts_legacy_shape():
    # legacy payload has no ts either; v2 must default it to 0
    out = h2({"user": "u"})
    assert out["ts"] == 0
    assert out["meta"] == {}
