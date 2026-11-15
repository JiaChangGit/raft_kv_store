#!/usr/bin/env bash
# Lesson 15: the debug status embeds the whole key-value map. Above the 1 MiB
# frame limit the node cannot send it, the client sees the connection close,
# and every tool that finds the leader through the status stops working.
set -euo pipefail
LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
mkdir -p "$ROOT/artifacts/runs"
WORK="$(mktemp -d "$ROOT/artifacts/runs/lesson15.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
REF=s1-s4-review-verified
mkdir "$WORK/src"
git -C "$ROOT" archive "$REF" | tar -x -C "$WORK/src"
(cd "$WORK/src" && CARGO_TARGET_DIR="$WORK/target" cargo build -q --release --locked -p raft-node)
(cd "$ROOT" && cargo build -q --release --locked -p raft-node)
printf '\n== before the fix (%s)\n' "$REF"
"$ROOT/.venv/bin/python" "$LESSON/probe.py" "$WORK/target/release/raft-node" "$WORK" before
printf '\n== after the fix (current source)\n'
"$ROOT/.venv/bin/python" "$LESSON/probe.py" "$ROOT/target/release/raft-node" "$WORK" after
printf '\n== RESULT\n'
echo "Before: the status cannot be sent once the map exceeds the frame. After: it answers and says the map was left out."
