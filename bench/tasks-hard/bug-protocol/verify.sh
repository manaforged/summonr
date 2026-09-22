#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && [ "$(shasum -a 256 test_proto.py | cut -d' ' -f1)" = "8eafac4576bb67c0e71a299ea638ba22db9b4ccd0f52e7d39650df70c2cd9552" ]
