#!/usr/bin/env bash
# Lesson 01: reproduce the WAL length-corruption bug on the old format 1 code,
# then show the current code refusing the same damage.
# Everything is written under one temporary directory that is removed at exit.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
TAG=v1-baseline-verified-20261112
WORK="$(mktemp -d "${TMPDIR:-/tmp}/wal-lesson.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
export TMPDIR="$WORK"

say() { printf '\n== %s\n' "$*"; }
size() { stat -c %s "$1"; }
payloads() { grep -a -o '{"term":[^]]*\]}' "$1" | sed 's/^/     /'; }
# patch FILE OFFSET VALUE: overwrite a 4-byte little-endian integer in place.
patch() {
  local v=$3
  printf "$(printf '\\x%02x\\x%02x\\x%02x\\x%02x' $((v & 255)) $((v >> 8 & 255)) $((v >> 16 & 255)) $((v >> 24 & 255)))" |
    dd of="$1" bs=1 seek="$2" conv=notrunc status=none
}

say "1. Build the old code (git tag $TAG) in a temporary copy"
mkdir "$WORK/old"
git -C "$ROOT" archive "$TAG" | tar -x -C "$WORK/old"
(cd "$WORK/old" && CARGO_TARGET_DIR="$WORK/target-old" cargo build -q --locked -p raft-storage --bin wal_probe)
OLD="$WORK/target-old/debug/wal_probe"
echo "   built: wal_probe (format 1)"

say "2. Write two durable records with the old code: term 1, then term 2"
"$OLD" "$WORK/good" 1
"$OLD" "$WORK/good" 2
LEN=$(( $(size "$WORK/good/raft.wal") / 2 - 24 ))
echo "   raft.wal is $(size "$WORK/good/raft.wal") bytes: 2 records, each 20 (header) + $LEN (payload) + 4 (CRC)"
payloads "$WORK/good/raft.wal"

say "3. Case A: damage the FIRST record's length field ($LEN -> 65536), then restart with term 1"
cp -r "$WORK/good" "$WORK/a"
patch "$WORK/a/raft.wal" 8 65536
if "$OLD" "$WORK/a" 1; then
  echo "   the old code started normally. raft.wal is now $(size "$WORK/a/raft.wal") bytes:"
  payloads "$WORK/a/raft.wal"
else
  echo "NOT REPRODUCED: the old code refused case A"; exit 1
fi
[ "$(size "$WORK/a/raft.wal")" -eq $((24 + LEN)) ] || { echo "NOT REPRODUCED: unexpected size"; exit 1; }
echo "BUG REPRODUCED (A): the node had durably reached term 2, forgot both records, and accepted term 1 again."

say "4. Case B: damage the LAST record's length field ($LEN -> $((LEN + 1))), then restart with term 3"
cp -r "$WORK/good" "$WORK/control"
"$OLD" "$WORK/control" 3 >/dev/null
echo "   control (no damage), after writing term 3:"
payloads "$WORK/control/raft.wal"
cp -r "$WORK/good" "$WORK/b"
patch "$WORK/b/raft.wal" $((24 + LEN + 8)) $((LEN + 1))
"$OLD" "$WORK/b" 3 >/dev/null || { echo "NOT REPRODUCED: the old code refused case B"; exit 1; }
echo "   damaged, after writing term 3:"
payloads "$WORK/b/raft.wal"
[ "$(size "$WORK/b/raft.wal")" -lt "$(size "$WORK/control/raft.wal")" ] || { echo "NOT REPRODUCED: nothing was lost"; exit 1; }
echo "BUG REPRODUCED (B): the durable entry (index 2, term 2) is gone; index 2 now holds a term 3 entry."

say "5. Run a regression test against the old code (it should FAIL)"
cp "$LESSON/v1_red_test.rs" "$WORK/old/crates/raft-storage/tests/length_corruption.rs"
if (cd "$WORK/old" && CARGO_TARGET_DIR="$WORK/target-old" cargo test -q --locked -p raft-storage --test length_corruption >"$WORK/red.log" 2>&1); then
  echo "UNEXPECTED: the regression test passed on the old code"; exit 1
fi
grep -E "the damaged WAL|test result" "$WORK/red.log" | sed 's/^/   /'
echo "RED: the test fails on format 1, so it really detects this bug."

say "6. Build the current code and repeat both cases"
(cd "$ROOT" && CARGO_TARGET_DIR="$WORK/target-new" cargo build -q --locked -p raft-storage --bin wal_probe)
NEW="$WORK/target-new/debug/wal_probe"
"$NEW" "$WORK/new" 1
"$NEW" "$WORK/new" 2
NLEN=$(( ($(size "$WORK/new/raft.wal") - 80) / 2 - 28 ))
echo "   raft.wal is $(size "$WORK/new/raft.wal") bytes: 80 (file header) + 2 records, each 24 (header) + $NLEN (payload) + 4 (CRC)"
for case in A B; do
  cp -r "$WORK/new" "$WORK/new-$case"
  if [ "$case" = A ]; then patch "$WORK/new-$case/raft.wal" $((80 + 8)) 65536
  else patch "$WORK/new-$case/raft.wal" $((80 + 28 + NLEN + 8)) $((NLEN + 1)); fi
  cp "$WORK/new-$case/raft.wal" "$WORK/damaged-$case"
  if "$NEW" "$WORK/new-$case" 3 2>"$WORK/err-$case"; then
    echo "NOT FIXED: the current code accepted case $case"; exit 1
  fi
  echo "   case $case: refused -> $(grep -o '"[^"]*"' "$WORK/err-$case" | tail -1)"
  cmp -s "$WORK/new-$case/raft.wal" "$WORK/damaged-$case" || { echo "NOT FIXED: the file was modified"; exit 1; }
  [ ! -e "$WORK/new-$case/torn" ] || { echo "NOT FIXED: something was cut off"; exit 1; }
  echo "            raft.wal is byte-for-byte unchanged"
done
echo "FIXED: format 2 refuses both cases and does not touch the file."

say "RESULT"
echo "Bug reproduced on format 1 (cases A and B); regression test is red there; format 2 refuses both."
