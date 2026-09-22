from models import Record
from util import format_id, make


def test_record_display():
    assert Record(7, "x").display() == "0007:x"


def test_util_make():
    r = make("y")
    assert isinstance(r, Record)
    assert r.name == "y"


def test_format_id():
    assert format_id(42) == "0042"
