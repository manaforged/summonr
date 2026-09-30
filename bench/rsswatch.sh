#!/usr/bin/env bash
set -u
pid=${1:?usage: rsswatch.sh PID OUT [SECS]}
out=${2:?usage: rsswatch.sh PID OUT [SECS]}
limit=${3:-0}
grace=10
echo "rss_kb parent tree children" > "$out"
end=$((SECONDS + limit))
termed=0
sample() {
    ps -A -o pid=,ppid=,rss=,comm= 2>/dev/null | awk -v root="$pid" -v match_name="${RSSWATCH_COUNT:-}" '
        { parent[$1] = $2; rss[$1] = $3; name[$1] = $4 }
        END {
            if (!(root in rss)) exit
            tree[root] = 1
            grown = 1
            while (grown) {
                grown = 0
                for (p in parent) if (!(p in tree) && (parent[p] in tree)) { tree[p] = 1; grown = 1 }
            }
            total = 0
            members = ""
            count = 0
            for (p in tree) {
                total += rss[p]
                members = members " " p
                if (p != root && (match_name == "" || name[p] ~ match_name)) count++
            }
            print rss[root], total, count, members
        }'
}
while kill -0 "$pid" 2>/dev/null; do
    line=$(sample)
    [ -n "$line" ] && echo "$line" | awk '{print $1, $2, $3}' >> "$out"
    if [ "$limit" -gt 0 ] && [ "$SECONDS" -ge "$end" ]; then
        if [ "$termed" -eq 0 ]; then
            kill -TERM "$pid" 2>/dev/null
            termed=1
        elif [ "$SECONDS" -ge $((end + grace)) ]; then
            kill -KILL $(echo "$line" | cut -d' ' -f4-) "$pid" 2>/dev/null
        fi
    fi
    sleep 0.25
done
