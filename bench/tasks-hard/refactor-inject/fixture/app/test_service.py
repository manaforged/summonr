from service import Service


def make():
    return Service({"retries": 2, "timeout": 10})


def test_describe_uses_injected_config():
    assert make().describe() == "retries=2 timeout=10"


def test_retry_boundary():
    s = make()
    assert s.will_retry(1)
    assert not s.will_retry(2)
