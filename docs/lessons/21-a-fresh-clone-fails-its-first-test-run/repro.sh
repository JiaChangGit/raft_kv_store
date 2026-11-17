#!/usr/bin/env bash
# Lesson 21: the first `cargo test` of a fresh clone can fail three raft-node
# tests with "File exists". Several WALs are opened at once under a parent
# directory that does not exist yet; whoever loses the race to create it gets
# an error. This script plants one racing test into the version before the fix
# and into the current source, in a temporary directory. About two minutes.
set -euo pipefail
LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
BEFORE=s9-public-verified
FILES=(crates config Cargo.toml Cargo.lock rust-toolchain.toml)
mkdir -p "$ROOT/artifacts/runs"
WORK="$(mktemp -d "$ROOT/artifacts/runs/lesson21.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

run() {  # run NAME: append the racing test to that copy and run it
  cat "$LESSON/race_test.rs" >> "$WORK/$1/crates/raft-storage/src/tests.rs"
  (cd "$WORK/$1" && CARGO_TARGET_DIR="$WORK/target-$1" cargo test -q --locked -p raft-storage \
     lesson_21_opens_racing -- --nocapture 2>&1) | grep '^LESSON' | sed 's/^LESSON /   /'
}

printf '\n== %s (before the fix)\n' "$BEFORE"
mkdir "$WORK/before"
git -C "$ROOT" archive "$BEFORE" "${FILES[@]}" | tar -x -C "$WORK/before"
run before

printf '\n== current source\n'
mkdir "$WORK/now"
tar -C "$ROOT" --exclude=target -c "${FILES[@]}" | tar -x -C "$WORK/now"
run now

printf '\n== the same race in a fresh copy of %s: raft-node tests\n' "$BEFORE"
(cd "$WORK/before" && git -C "$ROOT" archive "$BEFORE" crates/raft-storage/src/tests.rs | tar -x -C "$WORK/before" \
   && CARGO_TARGET_DIR="$WORK/target-before" cargo test -q --locked -p raft-node 2>&1) \
  | grep -E '^test result|File exists' | sort | uniq -c | sed 's/^ */   /' || true

printf '\n== RESULT\n'
echo "Before the fix some opens lose the race and are refused; after it none are."
