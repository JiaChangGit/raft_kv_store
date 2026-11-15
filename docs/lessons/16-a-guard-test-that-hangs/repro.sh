#!/usr/bin/env bash
# Lesson 16: plant the same mistake (every RPC deadline already over) in three
# versions and run the guard test saturated_outbound_pool_does_not_starve_status.
# S4 fails it in about two seconds. S5 before the fix neither fails nor passes:
# it waits for ever while the node prints tens of MB per second. Every run goes
# through scripts/capped_run.py (60 s, 256 MiB), so nothing can fill the machine.
set -euo pipefail
LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
mkdir -p "$ROOT/artifacts/runs"
WORK="$(mktemp -d "$ROOT/artifacts/runs/lesson16.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
FILES=(crates config Cargo.toml Cargo.lock rust-toolchain.toml)
copy() {  # name, git ref or "current"
  mkdir -p "$WORK/$1"
  if [ "$2" = current ]; then
    tar -C "$ROOT" -c "${FILES[@]}" | tar -x -C "$WORK/$1"
  else
    git -C "$ROOT" archive "$2" "${FILES[@]}" | tar -x -C "$WORK/$1"
  fi
}
copy s4 s1-s4-review-verified
copy s5 e5f09c2
copy now current
for name in s4 s5 now; do
  "$ROOT/.venv/bin/python" "$LESSON/probe.py" plant "$WORK/$name"
  (cd "$WORK/$name" && CARGO_TARGET_DIR="$WORK/target-$name" cargo test -q --locked -p raft-node --no-run 2>/dev/null)
done
"$ROOT/.venv/bin/python" "$LESSON/probe.py" run "$WORK" \
  "s4:S4 (s1-s4-review-verified)" "s5:S5 before the fix (e5f09c2)" "now:current source"
