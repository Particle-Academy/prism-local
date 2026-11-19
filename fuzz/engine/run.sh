#!/usr/bin/env bash
# Runs a time-boxed campaign, at most two engine processes at once.
# usage: fuzz/engine/run.sh <seconds-per-target> [work-dir]
# First proves the engine can find the planted canary, then fuzzes every
# corpus target. Exits nonzero if the canary is missed, or if any target
# panics or has an input slower than one second. Results stay in work-dir.
set -uo pipefail
SECONDS_PER_TARGET=${1:?seconds per target}
WORK=${2:-target/fuzz-work}
ENGINE=target/fuzz-cov/engine
[ -x "$ENGINE.exe" ] && ENGINE="$ENGINE.exe"
if [ ! -x "$ENGINE" ]; then
  echo "::error::no fuzz engine at $ENGINE; run fuzz/engine/build.sh first"
  exit 2
fi
mkdir -p "$WORK/canary-seed"
printf 'hello world!' > "$WORK/canary-seed/seed"
status=0

if ! "$ENGINE" canary "$WORK/canary-seed" "$WORK/canary" 60 1 > "$WORK/canary.json" 2> "$WORK/canary.log"; then
  echo "::error::fuzz engine did not find the planted canary within 60 s"
  status=1
fi
cat "$WORK/canary.json"

targets=$(ls fuzz/corpus)
set -- $targets
while [ $# -gt 0 ]; do
  pids=""
  for t in "$1" "${2:-}"; do
    [ -n "$t" ] || continue
    "$ENGINE" "$t" "fuzz/corpus/$t" "$WORK/$t" "$SECONDS_PER_TARGET" 1 \
      > "$WORK/$t.json" 2> "$WORK/$t.log" &
    pids="$pids $!:$t"
  done
  for entry in $pids; do
    if ! wait "${entry%%:*}"; then
      echo "::error::fuzz target ${entry#*:} found a crash or slow input"
      status=1
    fi
    cat "$WORK/${entry#*:}.json"
  done
  shift; [ $# -gt 0 ] && shift
done
exit $status
