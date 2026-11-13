#!/usr/bin/env bash
# Lesson 07: S3 changed how GETs work, and a test that used a GET to fill the
# outbound connection pool silently stopped filling it. It stayed green; only
# a mutation run noticed. Builds three versions in a temporary directory.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/silent-test.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
say() { printf '\n== %s\n' "$*"; }
TEST=saturated_outbound_pool_does_not_starve_status
ANCHOR=$'                            Instant::now() + PEER_TIMEOUT,\n                        );'
MUTANT=$'                            Instant::now(),\n                        );'

run() {  # run REF NAME MUTATE: build REF, optionally plant the mistake, run the test
  local dir="$WORK/$2"
  mkdir "$dir"
  git -C "$ROOT" archive "$1" | tar -x -C "$dir"
  if [ "$3" = yes ]; then
    python3 -I - "$dir/crates/raft-node/src/main.rs" "$ANCHOR" "$MUTANT" <<'PY'
import sys
path, old, new = sys.argv[1:]
text = open(path).read()
assert text.count(old) == 1, "anchor not found"
open(path, "w").write(text.replace(old, new, 1))
PY
  fi
  (cd "$dir" && CARGO_TARGET_DIR="$dir/target" cargo test -q --locked -p raft-node "$TEST" \
     >"$dir.out" 2>"$dir.err") && echo pass || echo FAIL
}
sends() { grep -c '"kind":"rpc_send"' "$WORK/$1.out" || true; }

for ref in s2-snapshot-compaction-verified 54ff116 HEAD; do
  case $ref in
    s2-*) what="before S3: the test sends a GET, and a GET is replicated" ;;
    54ff116) what="after S3, before the fix: the test sends a GET, a GET is a ReadIndex read" ;;
    HEAD) what="current: the test sends a Put" ;;
  esac
  say "$ref  ($what)"
  CONTROL=$(run "$ref" "control-$ref" no)
  echo "   control (no mistake planted): test $CONTROL; RPCs the node tried to send: $(sends "control-$ref")"
  MUTANT_RESULT=$(run "$ref" "mutant-$ref" yes)
  if [ "$MUTANT_RESULT" = FAIL ]; then echo "   with the busy-retry mistake planted: test FAIL -> mistake detected"
  else echo "   with the busy-retry mistake planted: test pass -> mistake NOT detected"; fi
done

say "RESULT"
echo "The same test, unchanged between the first two versions, guarded the retry rule before S3 and not after."
