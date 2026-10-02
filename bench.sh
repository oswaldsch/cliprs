#!/usr/bin/env bash
set -euo pipefail

DURATION=${1:-20}
WARMUP=3
BIN=./target/release/cliprs-daemon

cd "$(dirname "$0")"

if [[ -n ${BENCH_PID:-} ]]; then
    pid=$BENCH_PID
    read_fdinfo() { cat /proc/"$pid"/fdinfo/* 2>/dev/null || true; }
else
    cargo build --release -p cliprs-daemon
    sudo -v
    log=$(mktemp)
    sudo "$BIN" >"$log" 2>&1 &
    trap 'sudo pkill -x cliprs-daemon || true; rm -f "$log"' EXIT
    sleep "$WARMUP"
    if ! pid=$(pgrep -n -x cliprs-daemon); then
        echo "cliprs-daemon exited during startup:"
        cat "$log"
        exit 1
    fi
    read_fdinfo() { sudo sh -c "cat /proc/$pid/fdinfo/*" 2>/dev/null || true; }
fi

# Several fds can point at the same DRM client, so values are deduplicated by client id.
drm_stats() {
    read_fdinfo | awk '
        /^pos:/ { id = "" }
        /^drm-client-id:/ { id = $2 }
        /^drm-engine-/ && id != "" {
            name = $1
            sub(/^drm-engine-/, "", name)
            sub(/:$/, "", name)
            engine[id SUBSEP name] = $2
            names[name]
        }
        /^drm-memory-vram:/ && id != "" { vram[id] = $2 }
        END {
            for (n in names) {
                total = 0
                for (k in engine) {
                    split(k, part, SUBSEP)
                    if (part[2] == n) total += engine[k]
                }
                print "engine", n, total
            }
            total = 0
            for (id in vram) total += vram[id]
            print "vram", total
        }'
}

cpu_ticks() { awk '{ print $14 + $15 }' /proc/"$pid"/stat; }

before=$(drm_stats)
cpu_before=$(cpu_ticks)
start=$(date +%s.%N)
echo "measuring pid $pid for $DURATION s"
sleep "$DURATION"
after=$(drm_stats)
cpu_after=$(cpu_ticks)
elapsed=$(awk -v s="$start" -v e="$(date +%s.%N)" 'BEGIN { print e - s }')
rss_kib=$(awk '/^VmRSS:/ { print $2 }' /proc/"$pid"/status)
peak_kib=$(awk '/^VmHWM:/ { print $2 }' /proc/"$pid"/status)

echo
awk -v t="$elapsed" -v ticks=$((cpu_after - cpu_before)) -v hz="$(getconf CLK_TCK)" \
    'BEGIN { printf "%-10s %6.1f %% of one core\n", "cpu", 100 * ticks / hz / t }'
{
    echo "$before" | sed 's/^/before /'
    echo "$after" | sed 's/^/after /'
} | awk -v t="$elapsed" '
    $1 == "before" && $2 == "engine" { start[$3] = $4 }
    $1 == "after" && $2 == "engine" { end[$3] = $4 }
    $1 == "after" && $2 == "vram" { vram = $3 }
    END {
        for (n in end) printf "%-10s %6.1f %%\n", "gpu " n, 100 * (end[n] - start[n]) / 1e9 / t
        printf "%-10s %6.0f MiB\n", "vram", vram / 1024
    }' | sort
awk -v kib="$rss_kib" -v peak="$peak_kib" 'BEGIN {
    printf "%-10s %6.0f MiB\n", "rss", kib / 1024
    printf "%-10s %6.0f MiB\n", "rss peak", peak / 1024
}'
