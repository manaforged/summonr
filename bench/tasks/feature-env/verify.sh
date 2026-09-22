cd app && \
  [ "$(MYAPP_DIR=/tmp/x python3 dirs.py)" = "/tmp/x" ] && \
  [ "$(env -u MYAPP_DIR python3 dirs.py)" = "$HOME/.myapp" ] && \
  [ "$(MYAPP_DIR='~/z' python3 dirs.py)" = "$HOME/z" ]
