#!/usr/bin/env bash
# Lesson 06: two copies of the same workspace share one cargo target directory.
# The second copy's source says QUORUM = 2, but its test runs the first copy's
# QUORUM = 3, because cargo judged the reused build "Fresh".
# Writes only under one temporary directory, removed at exit.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/shared-target.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
say() { printf '\n== %s\n' "$*"; }
TEST=quorum_is_two_of_three
# The lesson was written during S3. S4 removed the fixed QUORUM constant this script
# edits, so it copies that stage's tree rather than HEAD.
TAG=s3-read-index-verified

# copy NAME PRESERVE: copy the workspace; PRESERVE=yes keeps old modification
# times like Python's shutil.copytree (copy2) did in scripts/mutation_check.py.
copy() {
  mkdir "$WORK/$1"
  git -C "$ROOT" archive "$TAG" | tar -x -C "$WORK/$1" --touch
  if [ "$2" = yes ]; then
    # Give every file the modification time it has in the original checkout.
    (cd "$WORK/$1" && find . -type f | while read -r f; do
       [ -e "$ROOT/$f" ] && touch -r "$ROOT/$f" "$f"; done)
  fi
}
test_in() {  # test_in COPY TARGET: run the quorum test, print what cargo did
  (cd "$WORK/$1" && CARGO_TARGET_DIR="$WORK/$2" cargo test -v --locked -p raft-sim --test foundation "$TEST" \
     >"$WORK/$1-$2.out" 2>"$WORK/$1-$2.err") && result=pass || result=FAIL
  state=$(grep -oE "(Fresh|Compiling) raft-protocol" "$WORK/$1-$2.err" | head -1)
  echo "   copy $1, target dir '$2': cargo says '${state:-?}'; test $result"
}

say "1. Copy 'mutant': QUORUM changed to 3 (file written now)"
copy mutant no
grep -q "pub const QUORUM: usize = 2;" "$WORK/mutant/crates/raft-protocol/src/lib.rs" \
  || { echo "the QUORUM line is not in $TAG; the lesson needs updating"; exit 1; }
sed -i 's/pub const QUORUM: usize = 2;/pub const QUORUM: usize = 3;/' "$WORK/mutant/crates/raft-protocol/src/lib.rs"
grep -n "pub const QUORUM" "$WORK/mutant/crates/raft-protocol/src/lib.rs" | sed 's/^/   source: /'
test_in mutant shared

say "2. Copy 'clean': untouched, old modification times kept, SAME target directory"
copy clean yes
grep -n "pub const QUORUM" "$WORK/clean/crates/raft-protocol/src/lib.rs" | sed 's/^/   source: /'
test_in clean shared
grep -h -A3 "panicked" "$WORK/clean-shared.out" "$WORK/clean-shared.err" 2>/dev/null | head -3 | sed 's/^/   /' || true

say "3. The same clean copy with its OWN target directory"
test_in clean own

say "4. Why cargo reused it: both copies get the same -C metadata hash"
meta() { grep -E "crate-name raft_protocol " "$1" | grep -oE "metadata=[0-9a-f]+" | head -1; }
echo "   mutant (step 1):         $(meta "$WORK/mutant-shared.err")"
echo "   clean  (step 3, own dir): $(meta "$WORK/clean-own.err")"
[ "$(meta "$WORK/mutant-shared.err")" = "$(meta "$WORK/clean-own.err")" ] && echo "   identical: in a shared target directory they occupy the same files"

say "RESULT"
if grep -q "Fresh raft-protocol" "$WORK/clean-shared.err" && ! grep -q "test result: ok" "$WORK/clean-shared.out"; then
  echo "BUG REPRODUCED: the clean copy ran the mutant's raft-protocol and its quorum test failed."
else
  echo "NOT REPRODUCED"; exit 1
fi
grep -q "test result: ok" "$WORK/clean-own.out" && echo "FIXED: with its own target directory the clean copy passes." \
  || { echo "UNEXPECTED: the isolated clean copy failed"; exit 1; }
