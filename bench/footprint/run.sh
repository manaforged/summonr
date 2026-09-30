#!/usr/bin/env bash
set -eu
usage="usage: run.sh capture|check SUMMONR_BIN FIXTURE_BIN OUT_DIR [SCENARIO...]"
mode=${1:?$usage}
summonr=${2:?$usage}
fixture=${3:?$usage}
out=${4:?$usage}
shift 4
case "$mode" in capture|check) ;; *) echo "$usage" >&2; exit 2 ;; esac
here=$(cd "$(dirname "$0")" && pwd)
bench=$(dirname "$here")
summonr=$(cd "$(dirname "$summonr")" && pwd)/$(basename "$summonr")
fixture=$(cd "$(dirname "$fixture")" && pwd)/$(basename "$fixture")
mkdir -p "$out"
out=$(cd "$out" && pwd)

one() {
    local name=$1 home=$2 rundir=$3
    local work="$rundir/work"
    mkdir -p "$rundir" "$work" "$home"
    python3 "$here/gate.py" script "$name" "$rundir/script.json"
    python3 "$here/replay.py" --script "$rundir/script.json" --port-file "$rundir/port" --log "$rundir/replay.log" &
    local replay=$!
    local tries=0
    until [ -s "$rundir/port" ] || [ "$tries" -ge 100 ]; do
        sleep 0.1
        tries=$((tries + 1))
    done
    python3 "$here/gate.py" config "$name" "$home" "$rundir" "$(cat "$rundir/port")" "$fixture"
    git -C "$work" init -q
    git -C "$work" -c user.name=bench -c user.email=bench@localhost commit -q --allow-empty -m baseline
    touch "$rundir/started"
    (cd "$work" && exec env HOME="$home" SUMMONR_HOME="$home" SUMMONR_MODELS_URL="http://127.0.0.1:$(cat "$rundir/port")/registry.json" "$summonr" -e replay -p go) \
        >"$rundir/stdout" 2>"$rundir/stderr" </dev/null &
    local agent=$!
    RSSWATCH_COUNT='(^|/)mcp-[0-9a-f]+$' "$bench/rsswatch.sh" "$agent" "$rundir/rss" &
    local watcher=$!
    local status=0
    wait "$agent" || status=$?
    wait "$watcher" 2>/dev/null || true
    kill "$replay" 2>/dev/null || true
    wait "$replay" 2>/dev/null || true
    echo "$status" > "$rundir/exit"
    python3 "$here/gate.py" measure "$rundir" "$home"
    echo "$name $(basename "$rundir") exit $status $(cat "$rundir/metrics.json")"
}

names=()
while read -r name runs warm; do
    names+=("$name")
    rm -rf "${out:?}/$name"
    for i in $(seq 1 "$runs"); do
        home="$out/$name/run-$i/home"
        if [ "$warm" = 1 ]; then
            one "$name" "$home" "$out/$name/warm-$i" >/dev/null
        fi
        one "$name" "$home" "$out/$name/run-$i"
    done
done < <(python3 "$here/gate.py" plan "$@")

python3 "$here/gate.py" "$mode" "$out" "${names[@]}"
