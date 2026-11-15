#!/usr/bin/env bash
# Lesson 19: the S5 test has a case named "entry term below the previous entry",
# yet removing that rule from validate_delta passes the test: the case also
# breaks another rule, which then refuses it instead. The case S6 added breaks
# that rule alone. Under a minute.
set -euo pipefail
LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
mkdir -p "$ROOT/artifacts/runs"
WORK="$(mktemp -d "$ROOT/artifacts/runs/lesson19.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
FILES=(crates config Cargo.toml Cargo.lock rust-toolchain.toml)
TEST=invalid_deltas_are_refused_without_writing
remove_rule() {  # the mutation case s5_delta_entry_term_may_decrease
  "$ROOT/.venv/bin/python" - "$1/crates/raft-storage/src/lib.rs" <<'PY'
import sys
from pathlib import Path
path = Path(sys.argv[1]); text = path.read_text()
rule = "\n            || entry.term < previous"
assert text.count(rule) == 1, "the mutation anchor moved"
path.write_text(text.replace(rule, ""))
PY
}
which_rule() {  # build quietly (a removed rule leaves an unused variable), then run
  rm -rf "$WORK/wal" && mkdir "$WORK/wal"
  (cd "$WORK/s5" && CARGO_TARGET_DIR="$WORK/target-s5" cargo build -q --locked -p raft-storage --example which_rule 2>/dev/null)
  "$WORK/target-s5/debug/examples/which_rule" "$WORK/wal"; }
mkdir -p "$WORK/s5/crates/raft-storage/examples" "$WORK/now"
git -C "$ROOT" archive s5-performance-verified "${FILES[@]}" | tar -x -C "$WORK/s5"
cp "$LESSON/which_rule.rs" "$WORK/s5/crates/raft-storage/examples/"
tar -C "$ROOT" -c "${FILES[@]}" | tar -x -C "$WORK/now"

printf '\n== S5 (s5-performance-verified), every rule in place\n'
which_rule
remove_rule "$WORK/s5"
printf '\n== S5 with the rule "an entry term is never below the previous one" removed\n'
which_rule
if (cd "$WORK/s5" && CARGO_TARGET_DIR="$WORK/target-s5" cargo test -q --locked -p raft-storage "$TEST" > "$WORK/s5.out" 2>&1); then
  echo "   $TEST: passed, so the rule can go unnoticed"
else
  echo "   $TEST: FAILED (unexpected)"
fi

remove_rule "$WORK/now"
printf '\n== current source with the same rule removed\n'
if (cd "$WORK/now" && CARGO_TARGET_DIR="$WORK/target-now" cargo test -q --locked -p raft-storage "$TEST" > "$WORK/now.out" 2>&1); then
  echo "   $TEST: passed (unexpected)"
else
  echo "   $TEST: FAILED"
  grep -A1 "panicked at" "$WORK/now.out" | sed -n '2p' | sed 's/^/     /'
fi
printf '\n== RESULT\n'
echo "The S5 case is refused with or without the rule, so it never tested the rule. The S6 case needs it."
