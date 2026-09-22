#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && ! grep -rni 'legacy' . >/dev/null \
  && python3 cli.py --help | grep -vq 'legacy' \
  && python3 cli.py export a | grep -q '"name"'
