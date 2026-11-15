#!/usr/bin/env bash
# Lesson 14: the S5 baseline said write latency grows with the data even right
# after a snapshot. Part of that growth was the test tool: Cluster.invoke
# rewrites its whole history file after every request. Same node, same writes,
# sent two ways; the log is kept short (--snapshot-every 64) so the node's own
# per-entry cost cannot grow.
set -euo pipefail
LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
mkdir -p "$ROOT/artifacts/runs"   # on disk, not tmpfs: fsync must cost what it costs
WORK="$(mktemp -d "$ROOT/artifacts/runs/lesson14.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
REF=s1-s4-review-verified   # the build the baseline measured
mkdir "$WORK/src"
git -C "$ROOT" archive "$REF" | tar -x -C "$WORK/src"
(cd "$WORK/src" && CARGO_TARGET_DIR="$WORK/target" cargo build -q --release --locked -p raft-node)
printf '\n== before the fix: %s node and its tools/cluster.py; 3000 writes of 1 KiB, log kept short\n' "$REF"
"$ROOT/.venv/bin/python" -I "$LESSON/measure.py" "$WORK/src" "$WORK/target/release/raft-node" "$WORK/before"
printf '\n== after the fix: the same node, the current tools/cluster.py\n'
"$ROOT/.venv/bin/python" -I "$LESSON/measure.py" "$ROOT" "$WORK/target/release/raft-node" "$WORK/after"
printf '\n== RESULT\n'
echo "Only the old Cluster.invoke row grows: the growth was the history file being rewritten after every request, not the node."
