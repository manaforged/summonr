#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && ! git diff -U0 | grep -E '^\+.*\bimport\b' >/dev/null