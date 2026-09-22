import pytest
from loader_a import load as load_a
from loader_b import load as load_b


def test_loader_a_sees_only_its_plugin():
    reg = load_a()
    assert reg.run("a") == "A"
    with pytest.raises(KeyError):
        reg.run("b")


def test_loader_b_sees_only_its_plugin():
    reg = load_b()
    assert reg.run("b") == "B"
    with pytest.raises(KeyError):
        reg.run("a")
