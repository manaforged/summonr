#!/usr/bin/env bash
cd app && python3 -m pytest -q test_*.py >/dev/null 2>&1