#!/usr/bin/env bash
# Lesson 13: once the state machine no longer fits a snapshot (64 MiB), the node
# serialized all of it again after every single event, only to throw it away.
# Builds two versions in a temporary directory with a real 64 MiB state machine
# (needs about 1 GiB of memory); changes nothing in the repository.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/oversize-snapshot.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
say() { printf '\n== %s\n' "$*"; }
BEFORE=s4-membership-verified

run() {  # run NAME FIELDS REPORT: append the lesson's test to that version's actor and run it
  local dir="$WORK/$1"
  python3 -I - "$dir/crates/raft-node/src/main.rs" "$LESSON/oversize_actor.rs" "$2" "$3" <<'PY'
import sys
target, module, fields, report = sys.argv[1:]
text = open(module).read()
assert text.count("/*LESSON_FIELDS*/") == 1 and text.count("/*LESSON_REPORT*/") == 1
text = text.replace("/*LESSON_FIELDS*/", fields).replace("/*LESSON_REPORT*/", report)
open(target, "a").write(text)
PY
  (cd "$dir" && CARGO_TARGET_DIR="$dir/target" cargo test -q --locked -p raft-node \
     lesson_13_oversize_state_machine -- --nocapture >"$dir.out" 2>"$dir.err") || { cat "$dir.err"; exit 1; }
  grep '^LESSON' "$dir.out" | sed 's/^LESSON /   /'
}

say "$BEFORE  (before the fix)"
mkdir "$WORK/before"
git -C "$ROOT" archive "$BEFORE" | tar -x -C "$WORK/before"
run before 'snapshot_too_large: false,' ''

say "current source  (after the fix)"
mkdir "$WORK/after"
(cd "$ROOT" && git ls-files -co --exclude-standard -z | tar --null -T - -cf -) | tar -x -C "$WORK/after"
run after 'snapshot_limit: MAX_SNAPSHOT, snapshot_attempts: 0, oversize_retry_at: 0,' \
  'println!("LESSON serializations: {}; next attempt at applied index {}", actor.snapshot_attempts, actor.oversize_retry_at);'

say "RESULT"
before=$(grep -Ec 'snapshot check took +[1-9][0-9]+ ms' "$WORK/before.out" || true)
after=$(grep -Ec 'snapshot check took +[1-9][0-9]+ ms' "$WORK/after.out" || true)
echo "Events (out of 10) whose snapshot check took 10 ms or more: before the fix $before, after the fix $after."
if [ "$before" -eq 10 ] && [ "$after" -eq 1 ]; then
  echo "BUG REPRODUCED at $BEFORE: every event pays for a full serialization that is thrown away."
else
  echo "UNEXPECTED: see the lines above"; exit 1
fi
