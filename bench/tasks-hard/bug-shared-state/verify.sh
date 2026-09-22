#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && [ "$(shasum -a 256 test_plugins.py | cut -d' ' -f1)" = "c8c1f3095f579503f558b5a6324d4f9082ed8ebab642f3024a810e6af91c6786" ]
