#!/usr/bin/env bash
# Lesson 11: a follower installs, from a single message, a snapshot whose term
# is newer than the term in its own WAL, and is killed between "snapshot file
# durable" and "WAL rewritten". Before the fix it could never start again.
# Builds two versions in a temporary directory; changes nothing in the repository.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/newer-term-install.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
say() { printf '\n== %s\n' "$*"; }
BEFORE=s4-membership-verified

run() {  # run NAME: build the copy in $WORK/NAME with the lesson's test and print its story
  local dir="$WORK/$1"
  cp "$LESSON/interrupted_install.rs" "$dir/crates/raft-storage/tests/lesson_11.rs"
  (cd "$dir" && CARGO_TARGET_DIR="$dir/target" cargo test -q --locked -p raft-storage \
     --test lesson_11 -- --nocapture >"$dir.out" 2>"$dir.err") || { cat "$dir.err"; exit 1; }
  grep '^LESSON' "$dir.out" | sed 's/^LESSON /   /'
}

say "$BEFORE  (before the fix)"
mkdir "$WORK/before"
git -C "$ROOT" archive "$BEFORE" | tar -x -C "$WORK/before"
run before

say "current source  (after the fix)"
mkdir "$WORK/after"
# The files as they are on disk now (tracked, or new and not ignored).
(cd "$ROOT" && git ls-files -co --exclude-standard -z | tar --null -T - -cf -) | tar -x -C "$WORK/after"
run after

say "RESULT"
if grep -q 'restart 2: REFUSED' "$WORK/before.out" && grep -q 'restart 2: STARTED' "$WORK/after.out"; then
  echo "BUG REPRODUCED at $BEFORE: the node refuses to start, every time."
  echo "Current source: the node starts, in the snapshot's term and without a vote."
else
  echo "UNEXPECTED: see the lines above"; exit 1
fi
