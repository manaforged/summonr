#!/usr/bin/env bash
grep -q 'tools=${BENCH_TOOLS:-/opt/agent}' bench/harness.sh \
  && [ "$(git diff --stat | tail -1 | grep -oE '[0-9]+ file' | grep -oE '[0-9]+')" = "1" ]
