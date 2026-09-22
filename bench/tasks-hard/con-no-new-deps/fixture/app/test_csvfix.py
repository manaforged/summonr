from csvfix import parse_line


def test_simple():
    assert parse_line("a,b,c") == ["a", "b", "c"]


def test_quoted_comma():
    assert parse_line('a,"b,c",d') == ["a", "b,c", "d"]


def test_quoted_with_spaces():
    assert parse_line('x, "y,z" ,w') == ["x", "y,z", "w"]
