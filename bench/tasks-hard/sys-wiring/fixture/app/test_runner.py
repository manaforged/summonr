from runner import run


def test_run_returns_done():
    assert run("x") == "done: x"
