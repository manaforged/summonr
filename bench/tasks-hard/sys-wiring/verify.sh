#!/usr/bin/env bash
cd app && python3 cli.py job | grep -q '^done: job$' \
  && python3 cli.py --verbose job | grep -q 'engine: starting job' \
  && python3 cli.py job | grep -vq 'engine' \
  && python3 -m pytest -q test_*.py >/dev/null 2>&1