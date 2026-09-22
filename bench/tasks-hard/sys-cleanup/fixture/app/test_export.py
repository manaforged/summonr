from exporter import export


def test_export_json():
    assert export("a") == '{"name": "a"}'
