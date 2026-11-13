#!/usr/bin/env bash
# Lesson 10: a planted mistake "survived" the mutation test, but the test was
# fine: the mistake did not change what the program does. Builds three versions.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/equivalent-mutant.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
say() { printf '\n== %s\n' "$*"; }
REF=62e61d1
TEST=removing_a_follower_and_then_the_leader
FILE=crates/raft-core/src/lib.rs
# First attempt: also put the leader itself among the holders of entry n.
A_OLD=$'            let holders = voters\n                .ids()'
A_NEW=$'            let holders = voters\n                .with(self.id)\n                .ids()'
# Second attempt: a leader outside its configuration adds its own durable copy.
B_OLD='if self.state.term_at(n) == Some(self.state.term) && voters.is_quorum(holders) {'
B_NEW='if self.state.term_at(n) == Some(self.state.term) && holders.len() + usize::from(!voters.contains(self.id) && self.durable_index >= n) >= voters.majority() {'

run() {  # run NAME OLD NEW: build REF, replace OLD by NEW (if given), run the test
  local dir="$WORK/$1"
  mkdir "$dir"
  git -C "$ROOT" archive "$REF" | tar -x -C "$dir"
  if [ -n "$2" ]; then
    python3 -I - "$dir/$FILE" "$2" "$3" <<'PY'
import sys
path, old, new = sys.argv[1:]
text = open(path).read()
assert text.count(old) == 1, "anchor not found"
open(path, "w").write(text.replace(old, new, 1))
PY
  fi
  (cd "$dir" && CARGO_TARGET_DIR="$dir/target" cargo test -q --locked -p raft-core --lib "$TEST" \
     >"$dir.out" 2>"$dir.err") && echo pass || echo FAIL
}

say "control: nothing planted"
CONTROL=$(run control "" "")
echo "   test $TEST: $CONTROL"
say "mutant A: the leader is added to the holders of entry n"
A=$(run mutant-a "$A_OLD" "$A_NEW")
echo "   test $TEST: $A$([ "$A" = pass ] && echo '  -> survived')"
say "mutant B: a leader outside its configuration counts its own durable copy"
B=$(run mutant-b "$B_OLD" "$B_NEW")
echo "   test $TEST: $B$([ "$B" = FAIL ] && echo '  -> detected')"
grep -h "panicked at\|its own copy does not count" "$WORK/mutant-b.out" "$WORK/mutant-b.err" | head -2 | sed 's/^/   /' || true

say "why A changes nothing: the majority counts members only"
grep -n -A2 "pub fn is_quorum" "$WORK/control/crates/raft-protocol/src/lib.rs" | sed 's/^/   raft-protocol lib.rs:/'

say "RESULT"
if [ "$CONTROL" = pass ] && [ "$A" = pass ] && [ "$B" = FAIL ]; then
  echo "EQUIVALENT MUTANT SHOWN: the same test lets A pass and catches B; A never changed the program's behaviour"
else
  echo "unexpected: control=$CONTROL A=$A B=$B"
fi
