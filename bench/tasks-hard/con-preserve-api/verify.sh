#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && python3 - <<'PY'
import ast, pathlib
def sigs(src):
    return {(n.name, tuple(a.arg for a in n.args.args)) for n in ast.parse(src).body
            if isinstance(n, ast.FunctionDef)}
cur = sigs(pathlib.Path("tok.py").read_text())
want = {("tokenize", ("text",)), ("detokenize", ("tokens", "sep"))}
assert want <= cur, f"missing/renamed: {want - cur}"
PY
