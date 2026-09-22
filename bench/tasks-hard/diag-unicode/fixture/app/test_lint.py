from lint import check, report


def test_ascii_positions():
    assert check(["ok", "this line is way too long"], limit=10) == [(2, 25)]


def test_multibyte_report():
    # "héllo wörld" is 11 characters; the byte length is 13 but the limit is
    # in characters, so it must not be flagged.
    assert check(["héllo wörld"], limit=15) == []
    assert check(["héllo wörld"], limit=10) == [(1, 11)]
