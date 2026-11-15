#!/usr/bin/env bash
# Lesson 18: S5 raised the follower's limit for one AppendEntries from 32 to 256
# entries, and no test guarded it. Put it back to 32 in the S5 code: every core
# and protocol test still passes, yet a follower 100 entries behind never catches
# up. The test S6 added fails on the same mistake. About a minute.
set -euo pipefail
LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
mkdir -p "$ROOT/artifacts/runs"
WORK="$(mktemp -d "$ROOT/artifacts/runs/lesson18.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
FILES=(crates config Cargo.toml Cargo.lock rust-toolchain.toml)
plant() {  # the mutation case s5_follower_limit_still_32
  "$ROOT/.venv/bin/python" - "$1/crates/raft-core/src/lib.rs" <<'PY'
import sys
from pathlib import Path
path = Path(sys.argv[1]); text = path.read_text()
good = "let valid_entries = entries.len() <= MAX_BATCH_ENTRIES"
assert text.count(good) == 1, "the mutation anchor moved"
path.write_text(text.replace(good, "let valid_entries = entries.len() <= 32"))
PY
}
passed() { grep -Eo '[0-9]+ passed' | awk '{n += $1} END {print n + 0}'; }
mkdir -p "$WORK/s5" "$WORK/now"
git -C "$ROOT" archive s5-performance-verified "${FILES[@]}" | tar -x -C "$WORK/s5"
tar -C "$ROOT" -c "${FILES[@]}" | tar -x -C "$WORK/now"
plant "$WORK/s5"
plant "$WORK/now"

printf '\n== S5 (s5-performance-verified) with the limit back to 32\n'
count=$(cd "$WORK/s5" && CARGO_TARGET_DIR="$WORK/target-s5" cargo test -q --locked -p raft-core -p raft-protocol 2>&1 | passed)
echo "   raft-core and raft-protocol: $count tests, all passed"
(cd "$WORK/s5" && CARGO_TARGET_DIR="$WORK/target-s5" cargo build -q --locked -p raft-node)

printf '\n== current source with the same mistake\n'
if (cd "$WORK/now" && CARGO_TARGET_DIR="$WORK/target-now" cargo test -q --locked -p raft-core \
      a_follower_accepts_a_full_batch_and_refuses_one_more > "$WORK/now.out" 2>&1); then
  echo "   a_follower_accepts_a_full_batch_and_refuses_one_more: passed (unexpected)"
else
  echo "   a_follower_accepts_a_full_batch_and_refuses_one_more: FAILED"
  grep -A3 "panicked at" "$WORK/now.out" | sed -n '2,4p' | sed 's/^/     /'
fi
(cd "$ROOT" && cargo build -q --locked -p raft-node)

"$ROOT/.venv/bin/python" "$LESSON/probe.py" "$WORK/target-s5/debug/raft-node" "$WORK/cluster-s5" \
  "a real cluster, S5 with the limit back to 32"
"$ROOT/.venv/bin/python" "$LESSON/probe.py" "$ROOT/target/debug/raft-node" "$WORK/cluster-now" \
  "a real cluster, current source"
printf '\n== RESULT\n'
echo "With the limit at 32 the S5 tests pass and the follower never catches up; the S6 test fails on it."
