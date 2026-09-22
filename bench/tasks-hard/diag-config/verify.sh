#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1 \
  && [ "$(shasum -a 256 test_settings.py | cut -d' ' -f1)" = "4168318b7daddd2d9f283b7cc3b5ae584a5defc3ec9c3a43b0a4d1e38ad6a0dc" ]
