#!/usr/bin/env bash
# Summonr-internal A/B bench: same prompts, two binaries, journal-scored.
# Not part of the Harbor board — no proxy, no task suite, no cross-CLI table.
#
#   ab.sh OLD_BIN NEW_BIN TASKS_FILE [ENDPOINT]
#
# TASKS_FILE: one prompt per line (# comments and blanks skipped).
# Each prompt runs `summonr -e ENDPOINT -p` in its own detached worktree of
# this repo at HEAD, so edit/write tasks are safe and runs can overlap.
# AB_CONCURRENT (default 8) sets the pool width. Journals land in an
# isolated SUMMONR_HOME per arm; `profile --all` scores each arm.
set -euo pipefail

old=${1:?usage: ab.sh OLD_BIN NEW_BIN TASKS_FILE [ENDPOINT]}
new=${2:?usage: ab.sh OLD_BIN NEW_BIN TASKS_FILE [ENDPOINT]}
tasks=${3:?usage: ab.sh OLD_BIN NEW_BIN TASKS_FILE [ENDPOINT]}
endpoint=${4:-deepseek}
old=$(cd "$(dirname "$old")" && pwd)/$(basename "$old")
new=$(cd "$(dirname "$new")" && pwd)/$(basename "$new")
tasks=$(cd "$(dirname "$tasks")" && pwd)/$(basename "$tasks")
repo=$(cd "$(dirname "$0")/.." && pwd)
repeats=${AB_REPEATS:-1}
concurrent=${AB_CONCURRENT:-8}
out=$(mktemp -d)/ab
mkdir -p "$out/works"

run_one() {
    local bin=$1 home=$2 label=$3 task=$4 repeat=$5 prompt=$6
    local work="$out/works/$label-$task-$repeat"
    git -C "$repo" worktree add --detach "$work" HEAD >/dev/null 2>&1 || return
    (cd "$work" && SUMMONR_HOME="$home" "$bin" -e "$endpoint" -p "$prompt") \
        >"$out/$label-$task-$repeat.stdout" 2>&1 || true
    git -C "$repo" worktree remove --force "$work" >/dev/null 2>&1 || true
}

run_arm() {
    local bin=$1 home=$2 label=$3
    mkdir -p "$home"
    for auth in credentials.toml config.toml; do
        [ -f "$HOME/.summonr/$auth" ] && cp "$HOME/.summonr/$auth" "$home/$auth"
    done
    local task=0
    while IFS= read -r prompt; do
        case "$prompt" in ''|\#*) continue ;; esac
        task=$((task + 1))
        for repeat in $(seq "$repeats"); do
            while [ "$(jobs -pr | wc -l)" -ge "$concurrent" ]; do sleep 0.3; done
            run_one "$bin" "$home" "$label" "$task" "$repeat" "$prompt" &
        done
    done < "$tasks"
    wait
    SUMMONR_HOME="$home" "$new" profile --all >"$out/$label.profile"
}

run_arm "$old" "$out/old-home" old
run_arm "$new" "$out/new-home" new

{
    echo "ab bench $out"
    echo "endpoint $endpoint repeats $repeats concurrent $concurrent tasks $tasks"
    echo "old $old sha256 $(shasum -a 256 "$old" | cut -d' ' -f1)"
    echo "new $new sha256 $(shasum -a 256 "$new" | cut -d' ' -f1)"
    echo
    echo "== old =="
    cat "$out/old.profile"
    echo
    echo "== new =="
    cat "$out/new.profile"
} | tee "$out/table.txt"
