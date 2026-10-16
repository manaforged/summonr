#!/bin/bash
mkdir -p /logs/verifier
if cp /tmp/env.txt /logs/verifier/env.txt 2>/dev/null; then echo 1 > /logs/verifier/reward.txt; else echo 0 > /logs/verifier/reward.txt; fi
