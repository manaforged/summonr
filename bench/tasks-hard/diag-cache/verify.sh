#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && [ "$(shasum -a 256 test_store.py | cut -d' ' -f1)" = "c9cd5c95ef53f9fa90dddd1d86b89fe93a2c773e15acac46b3ddbb8e3c27553e" ]
