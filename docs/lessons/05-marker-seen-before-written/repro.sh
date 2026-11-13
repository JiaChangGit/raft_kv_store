#!/usr/bin/env bash
# Lesson 05: a test saw the failpoint marker file exist and read it before the
# program had written its content. Rebuilds the old in-place marker in a copy,
# widens its gap so the race always happens, and compares with the current code.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/marker-race.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
say() { printf '\n== %s\n' "$*"; }
TEST=real_process_crash_during_initialization_recovers

say "0. The failure seen once, in acceptance phase 11 on 2026-11-13"
grep -E "panicked|left|right" "$LESSON/observed-failure.txt" | sed 's/^/   /'

say "1. Two copies of the current code"
for copy in old new; do
  mkdir "$WORK/$copy"
  git -C "$ROOT" archive HEAD | tar -x -C "$WORK/$copy"
done
python3 -I - "$WORK/old/crates/raft-storage/src/lib.rs" <<'PY'
import sys
path = sys.argv[1]
text = open(path).read()
start = text.index('    if let Ok(marker) = std::env::var("RAFT_WAL_FAILPOINT_MARKER") {')
end = text.index("    loop {", start)
# What the code did until 2026-11-13: create the marker in place, then write it.
old = '''    if let Ok(marker) = std::env::var("RAFT_WAL_FAILPOINT_MARKER")
        && let Ok(mut file) = File::create(marker)
    {
        marker_failpoint_gap(); // the gap now sits between "exists" and "written"
        let _ = file.write_all(name.as_bytes());
        let _ = file.sync_all();
    }
'''
open(path, "w").write(text[:start] + old + text[end:])
PY
echo "   old: marker created in place, then written (as in tag v1-baseline-verified-20261112)"
echo "   new: written to marker.tmp, then renamed (current)"

run() {  # run COPY: the test with a 300 ms gap where each version has its gap
  if (cd "$WORK/$1" && RAFT_WAL_MARKER_GAP_MS=300 CARGO_TARGET_DIR="$WORK/target-$1" \
      cargo test -q --locked -p raft-storage --test crash_process "$TEST" >"$WORK/$1.out" 2>"$WORK/$1.err"); then
    echo pass
  else
    echo FAIL
  fi
}

say "2. Old version, 300 ms between creating and writing the marker"
OLD=$(run old)
echo "   result: $OLD"
grep -E "left|right" "$WORK/old.err" | head -2 | sed 's/^/   /' || true

say "3. Current version, 300 ms between writing and renaming the marker"
NEW=$(run new)
echo "   result: $NEW"

say "RESULT"
if [ "$OLD" = FAIL ]; then
  echo "BUG REPRODUCED: the test read an empty marker that existed but was not yet written."
else
  echo "NOT REPRODUCED"; exit 1
fi
if [ "$NEW" = pass ]; then
  echo "FIXED: with write-then-rename the marker is never visible half-written."
else
  echo "UNEXPECTED: the current version failed"; tail -20 "$WORK/new.err"; exit 1
fi
