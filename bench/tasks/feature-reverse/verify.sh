cd app && \
  [ "$(python3 tool.py a b c)" = "a
b
c" ] && \
  [ "$(python3 tool.py --reverse a b c)" = "c
b
a" ]
