from tok import tokenize, detokenize


def test_single_spaces():
    assert tokenize("a b c") == ["a", "b", "c"]


def test_multiple_spaces():
    assert tokenize("a  b") == ["a", "b"], "empty tokens must be dropped"


def test_detokenize_sep():
    assert detokenize(["a", "b"], sep=",") == "a,b"
