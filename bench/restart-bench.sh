#!/usr/bin/env bash
set -u
run_id=$1
shift
vendors=${*:-zai deepseek}
root=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$root/.." && pwd)
for p in $(pgrep -f "run.sh tb21-"); do kill "$p"; done
sleep 1
for p in $(pgrep -f "harbor run -p"); do kill -9 "$p"; done
sleep 3
docker ps -a --format "{{.Names}}" | grep "__" | xargs -r docker rm -f >/dev/null
docker network ls --format "{{.Name}}" | grep "__env" | while read -r n; do [ "$(docker network inspect -f "{{len .Containers}}" "$n")" = "0" ] && docker network rm "$n" >/dev/null; done
echo "env networks left: $(docker network ls --format "{{.Name}}" | grep -c __env)"
echo "left=$(pgrep -fc "harbor run -p")"
git -C "$repo" fetch -q origin && git -C "$repo" checkout -q -B main origin/main || exit 1
git -C "$repo" log --oneline -1
cd "$root" && ./tools.sh ./tools "$repo" > "logs/tools-$run_id.out" 2>&1
echo "tools exit $?"
grep -E "^(summonr_commit|node|pi) " tools/versions.txt
export BENCH_DATASET=${BENCH_DATASET:-$root/datasets/tb21/terminal-bench-2-1} BENCH_REPEATS=${BENCH_REPEATS:-1}
for vendor in $vendors; do
  BENCH_VENDOR=$vendor setsid nohup ./run.sh "tb21-$vendor-$run_id" tasks-tb21.txt summonr pi > "logs/tb21-$vendor-$run_id.out" 2>&1 < /dev/null &
  sleep 3
done
sleep 15
pgrep -af "harbor run -p" | grep -o "jobs/[^ ]*" | sort | uniq -c
