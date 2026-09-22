from pipeline import process, process_all


def test_sink_receives_each_item():
    got = []
    n = process([" a ", "b "], sink=got.append)
    assert got == ["A", "B"]
    assert n is None or isinstance(n, int)


def test_process_all_returns_results():
    assert process_all(["x", " y "]) == ["X", "Y"]
    assert process_all(["x"]) == ["X"]
