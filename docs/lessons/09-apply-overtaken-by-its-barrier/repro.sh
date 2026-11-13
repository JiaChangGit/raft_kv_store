#!/usr/bin/env bash
# Lesson 09: index 69 was applied before index 68. The durability barrier was
# placed first in a step's effects, and finishing it committed and applied a
# later entry before the step's own Apply ran. Builds two versions in a temp dir.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/apply-order.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
say() { printf '\n== %s\n' "$*"; }
REF=62e61d1
TEST=applies_stay_in_order_when_a_barrier_commits_more
# Moving nothing in front of the barrier is exactly the original order:
# barrier first, then every other effect in the order it was produced.
FIXED='            .partition(|effect| matches!(effect, Effect::Apply(_)));'
BEFORE='            .partition(|_| false);'
SEEDS=200

build() {  # build NAME PLANT
  local dir="$WORK/$1"
  mkdir "$dir"
  git -C "$ROOT" archive "$REF" | tar -x -C "$dir"
  if [ "$2" = yes ]; then
    python3 -I - "$dir/crates/raft-core/src/lib.rs" "$FIXED" "$BEFORE" <<'PY'
import sys
path, old, new = sys.argv[1:]
text = open(path).read()
assert text.count(old) == 1, "anchor not found"
open(path, "w").write(text.replace(old, new, 1))
PY
  fi
}
unit() {  # unit NAME
  (cd "$WORK/$1" && CARGO_TARGET_DIR="$WORK/$1/target" cargo test -q --locked -p raft-core --lib "$TEST" \
     >"$WORK/$1.out" 2>"$WORK/$1.err") && echo pass || echo FAIL
}
simulate() {  # simulate NAME: SEEDS seeds x 10000 events with the production core
  (cd "$WORK/$1" && CARGO_TARGET_DIR="$WORK/$1/target" cargo run -q --release --locked -p raft-sim -- \
     --seeds "$SEEDS" --events 10000 --output "$WORK/$1.sim.json" >/dev/null 2>&1) || true
  python3 -I - "$WORK/$1.sim.json" <<'PY'
import json, sys
report = json.load(open(sys.argv[1]))
print(f"   simulator: {report['pass_count']} of {report['seed_count']} seeds passed")
for failure in report["failures"][:1]:
    print(f"   first failure: seed {failure['seed']} after {failure['event']} events: {failure['invariant']}")
PY
}

build before yes
build after no
say "before the fix: the barrier goes first"
echo "   test $TEST: $(unit before)"
grep -hA2 "the applies precede the barrier" "$WORK/before.out" "$WORK/before.err" | sed 's/^/   /' || true
simulate before

say "after the fix ($REF): Applies first, then the barrier"
echo "   test $TEST: $(unit after)"
simulate after

say "why a real node would stop (from the source, not executed here)"
grep -n "queue.push_front(effect)" "$WORK/after/crates/raft-node/src/main.rs" | head -1 | sed 's/^/   raft-node main.rs:/'
grep -n '"apply must consume an uninterrupted committed prefix"' "$WORK/after/crates/kv-state/src/lib.rs" | sed 's/^/   kv-state lib.rs:/'

say "RESULT"
if grep -q '"persist",' "$WORK/before.out" "$WORK/before.err"; then
  echo "BUG REPRODUCED: the barrier ran before the step's Apply, and its completion applied a later index first"
else
  echo "not reproduced"
fi
