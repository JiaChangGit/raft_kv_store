#!/usr/bin/env bash
# Lesson 08: right after an election, a membership change was refused with
# CHANGE_IN_PROGRESS although nothing was in progress. The check ran before
# the new leader knew what was committed. Builds two versions in a temp dir.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/term-commit.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
say() { printf '\n== %s\n' "$*"; }
# The commit with the fix and its regression test; "before" puts back the
# original order of the checks (the fix was made before any commit had it).
REF=62e61d1
TEST=a_new_leader_waits_for_a_committed_entry_of_its_term
FIXED=$'        if self.pending_change.is_some() {\n            return effects.push(fail("CHANGE_IN_PROGRESS"));'
BEFORE=$'        let (index, _) = self.configuration().expect("a leader has a configuration");\n        if self.pending_change.is_some() || index > self.commit_index {\n            return effects.push(fail("CHANGE_IN_PROGRESS"));'

run() {  # run NAME PLANT: build REF, optionally restore the original check, run the test
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
  (cd "$dir" && CARGO_TARGET_DIR="$dir/target" cargo test -q --locked -p raft-core --lib "$TEST" \
     >"$dir.out" 2>"$dir.err") && echo pass || echo FAIL
}

say "before the fix: 'uncommitted configuration?' is judged first"
echo "   test $TEST: $(run before yes)"
grep -h "the request waits instead of failing" "$WORK/before.out" "$WORK/before.err" | sed 's/^/   /' || true

say "after the fix ($REF): wait for a committed entry of this term, then judge"
echo "   test $TEST: $(run after no)"

say "RESULT"
if grep -q 'CHANGE_IN_PROGRESS' "$WORK/before.out" "$WORK/before.err"; then
  echo "BUG REPRODUCED: a new leader with commit index 0 called the configuration at index 1 'in progress'"
else
  echo "not reproduced"
fi
