#!/usr/bin/env bash
cd pkg && grep -q '__version__ = "1.4.0"' __init__.py \
  && grep -q 'version = "1.4.0"' pyproject.toml \
  && head -5 CHANGELOG.md | grep -q '## 1.4.0' \
  && ! grep -rn '1.3.0' __init__.py pyproject.toml >/dev/null \
  && python3 -m pytest -q test_*.py >/dev/null 2>&1