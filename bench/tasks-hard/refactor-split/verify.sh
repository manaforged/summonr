#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && [ ! -f report.py ] && [ -f parse.py ] && [ -f render.py ] \
  && ! grep -rn 'import report\|from report' . >/dev/null \
  && [ "$(shasum -a 256 test_report.py | cut -d' ' -f1)" = "5c15169022e38c4f3e6e9fa8eef7803df682d1b4464528d54bd14e0dc15d88f5" ]
