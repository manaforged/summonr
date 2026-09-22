cd app && [ "$(python3 stats.py --json 1 2 4)" = '{"min":1.0,"mean":2.33,"max":4.0}' ] \
  && python3 stats.py 1 2 4 | grep -q "mean 2.33"
