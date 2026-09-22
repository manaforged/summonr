import json
import os
from settings import load


def test_file_loads(tmp_path):
    p = tmp_path / "c.json"
    p.write_text(json.dumps({"port": 8080}))
    assert load(str(p), env={})["port"] == 8080


def test_env_beats_file(tmp_path):
    p = tmp_path / "c.json"
    p.write_text(json.dumps({"port": 8080}))
    cfg = load(str(p), env={"APP_PORT": "9090"})
    assert cfg["port"] == "9090", "env must override file"
