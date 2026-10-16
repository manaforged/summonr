start_proxy() {
  local run=$1 port=$2 upstream=$3 envfile=$4 keyenv=$5
  shift 5
  mkdir -p "$root/logs"
  pkill -f "proxy.py .*--port $port .*--log $root/logs/$run.jsonl" || true
  for _ in $(seq 50); do (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null || break; sleep 0.2; done
  ! (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null || { echo "port $port is held by another run: $(pgrep -af "proxy.py .*--port $port " | grep -o -- '--log [^ ]*')" >&2; exit 1; }
  local keys=()
  if [ -n "$envfile" ]; then
    for name in $(grep -oE "^${keyenv}(_[0-9]+)?=" "$root/$envfile" | tr -d =); do keys+=(--key-env "$name"); done
  fi
  (set -a; [ -z "$envfile" ] || . "$root/$envfile"; set +a
    exec setsid nohup python3 "$root/proxy.py" --bind 0.0.0.0 --port "$port" "${keys[@]}" "$@" --log "$root/logs/$run.jsonl" ${BENCH_DUMP:+--dump "$root/logs/$run.bodies"} \
      --upstream "$upstream" >> "$root/logs/proxy-$run.out" 2>&1 < /dev/null) &
  for _ in $(seq 50); do (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null && break; sleep 0.2; done
  (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null || { echo "proxy did not start on $port: see logs/proxy-$run.out" >&2; exit 1; }
}
