#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && [ "$(shasum -a 256 test_handlers.py | cut -d' ' -f1)" = "6251d9ea09292c97c6c9a1f5d36325c648707b9dbcd9f7082988b71d7421f5e5" ]
