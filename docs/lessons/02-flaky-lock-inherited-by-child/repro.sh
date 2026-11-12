#!/usr/bin/env bash
# Lesson 02: a test that fails about once in thirty runs because a child process,
# forked by a *different* test, briefly holds a copy of this test's file lock.
# Writes only under one temporary directory, removed at exit.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
RUNS="${RUNS:-40}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/flaky-lock.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

say() { printf '\n== %s\n' "$*"; }

say "1. What lock does Wal::open take? (strace of the real binary)"
(cd "$ROOT" && cargo build -q --locked -p raft-storage --bin wal_probe)
if command -v strace >/dev/null; then
  strace -f -e trace=flock "$ROOT/target/debug/wal_probe" "$WORK/strace-node" 1 2>&1 | grep -E '^flock|DURABLE' | sed 's/^/   /'
else
  echo "   strace is not installed; skipping (the lesson text shows the captured line)"
fi

say "2. The mechanism, made deterministic: fork, then delay exec by one second"
python3 -I "$LESSON/flock_demo.py" "$WORK/demo.lock" | sed 's/^/   /'

say "3. The real test binary, $RUNS runs each, without and with the fix"
mkdir "$WORK/src"
git -C "$ROOT" archive HEAD | tar -x -C "$WORK/src"
TEST="$WORK/src/crates/raft-storage/tests/crash_process.rs"
cp "$TEST" "$WORK/fixed.rs"
grep -v 'let _serial = serial();' "$WORK/fixed.rs" > "$WORK/unfixed.rs"
count() {
  local fails=0
  cp "$1" "$TEST"
  (cd "$WORK/src" && CARGO_TARGET_DIR="$WORK/target" cargo test -q --locked -p raft-storage --test crash_process >/dev/null 2>&1) || true
  for _ in $(seq 1 "$RUNS"); do
    (cd "$WORK/src" && CARGO_TARGET_DIR="$WORK/target" cargo test -q --locked -p raft-storage --test crash_process >"$WORK/run.log" 2>&1) \
      || { fails=$((fails + 1)); cp "$WORK/run.log" "$WORK/last-failure.log"; }
  done
  echo "$fails"
}
UNFIXED=$(count "$WORK/unfixed.rs")
echo "   without serialization: $UNFIXED failures in $RUNS runs"
if [ -f "$WORK/last-failure.log" ]; then
  grep -m1 'already owned' "$WORK/last-failure.log" | sed 's/^/   example: /' || true
fi
rm -f "$WORK/last-failure.log"
FIXED=$(count "$WORK/fixed.rs")
echo "   with serialization:    $FIXED failures in $RUNS runs"

say "RESULT"
echo "Step 2 shows the cause deterministically. Step 3 is random by nature: zero failures"
echo "without the fix does not disprove the cause; any failure with the fix would be a new bug."
[ "$FIXED" -eq 0 ] || { echo "UNEXPECTED: the fixed test failed"; exit 1; }
