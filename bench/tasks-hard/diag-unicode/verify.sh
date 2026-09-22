#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && [ "$(shasum -a 256 test_lint.py | cut -d' ' -f1)" = "0066dfec91d18998a7219d28d94b433ba8baa5ae6f46ac76ca3e59e6dfa749b2" ]
