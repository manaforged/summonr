#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && [ "$(shasum -a 256 test_stats.py | cut -d' ' -f1)" = "cdcc3c1b5d623ba1c6e0a933ed73b593641d5bf09c046a734a54f8e165c9c61c" ]
