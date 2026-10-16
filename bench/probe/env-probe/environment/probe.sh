#!/bin/sh
echo "path $PATH"
echo "parent $(tr '\0' ' ' < /proc/$PPID/cmdline | cut -c1-160)"
for r in python3 python pip pip3 node npm npx bun uv uvx deno go java gcc cargo rustc ruby perl; do
  p=$(command -v "$r" 2>/dev/null)
  echo "run $r ${p:--} $([ -n "$p" ] && "$p" --version 2>&1 | head -n 1)"
done
