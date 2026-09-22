#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && [ "$(shasum -a 256 test_isbn.py | cut -d' ' -f1)" = "e86e94c1515bda698bda037f0131c1c7a9ca59941a52dd25452683a051ef9246" ]
