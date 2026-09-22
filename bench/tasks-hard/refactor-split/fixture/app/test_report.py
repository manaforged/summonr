from main import parse_rows, render_table


def test_parse():
    assert parse_rows("a=1\nb=2") == [("a", 1), ("b", 2)]


def test_render():
    out = render_table([("x", 3)])
    assert "x" in out and "TOTAL" in out and "3" in out
