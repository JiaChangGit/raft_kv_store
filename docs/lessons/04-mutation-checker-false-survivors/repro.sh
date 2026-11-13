#!/usr/bin/env bash
# Lesson 04: the mutation checker itself reported two mutants as "survived"
# although their tests had failed or never run. Rebuilds both situations in a
# temporary copy and judges them with the old and the current rules.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/mutation-check.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
say() { printf '\n== %s\n' "$*"; }

say "1. A copy of the current code"
mkdir "$WORK/src"
git -C "$ROOT" archive HEAD | tar -x -C "$WORK/src"

# plant FILE OLD NEW: replace exactly one occurrence, like mutation_check.py.
plant() {
  python3 -I - "$WORK/src/$1" "$2" "$3" <<'PY'
import sys
path, old, new = sys.argv[1], sys.argv[2], sys.argv[3]
text = open(path).read()
assert text.count(old) == 1, f"anchor occurs {text.count(old)} times"
open(path, "w").write(text.replace(old, new, 1))
PY
}
run_test() {  # run_test PACKAGE TEST OUT
  (cd "$WORK/src" && CARGO_TARGET_DIR="$WORK/target" cargo test -p "$1" "$2" \
     >"$WORK/$3.out" 2>"$WORK/$3.err") || true
}
judge() {  # judge TEST STDOUT_FILE STDERR_FILE
  python3 -I - "$ROOT/scripts" "$1" "$2" "$3" <<'PY'
import importlib.util, sys
scripts, test, out, err = sys.argv[1:]
sys.path.insert(0, scripts)  # mutation_check imports verify_gate from the same folder
spec = importlib.util.spec_from_file_location("m", f"{scripts}/mutation_check.py")
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
stdout, stderr = open(out).read(), open(err).read()
old = "detected" if f"{test} ... FAILED" in stdout else "SURVIVED"   # rule used until 2026-11-13
print(f"   old rule : {old}")
print(f"   new rule : {m.classify(test, stdout, stderr)}")
PY
}

TEST=saturated_outbound_pool_does_not_starve_status
: > "$WORK/empty.err"
say "2a. The two real outputs captured during development (deterministic)"
for n in 1 2; do
  echo "   observed-$n.txt: $(sed -n 1p "$LESSON/observed-$n.txt" | sed 's/^# Real output of the saturated_pool_spin mutant, //')"
  grep -m1 -E "^test tests::$TEST" "$LESSON/observed-$n.txt" | cut -c1-100 | sed 's/^/     /'
  judge "$TEST" "$LESSON/observed-$n.txt" "$WORK/empty.err"
done

say "2b. The same mutant run live now (interleaving is random; either outcome is possible)"
plant crates/raft-node/src/main.rs \
  $'                            Instant::now() + PEER_TIMEOUT,\n                        );' \
  $'                            Instant::now(),\n                        );'
run_test raft-node "$TEST" pool
grep -m1 -E "^test tests::$TEST" "$WORK/pool.out" | cut -c1-100 | sed 's/^/     /'
judge "$TEST" "$WORK/pool.out" "$WORK/pool.err"

say "3. A mutant written for an older version of the code: it does not compile"
plant crates/raft-storage/src/lib.rs \
  $'                return Err(io::Error::new(\n                    io::ErrorKind::NotFound,\n                    "identity exists but raft.wal is missing; refusing to assume an empty node",\n                ));' \
  '                write_atomically(&directory, "raft.wal", &file_header(node_id, PRISTINE))?;'
run_test raft-storage existing_identity_without_wal_fails_closed build
grep -m1 -E "^error\[" "$WORK/build.err" | sed 's/^/   compiler: /'
judge existing_identity_without_wal_fails_closed "$WORK/build.out" "$WORK/build.err"

say "4. An anchor that drifted away"
OLD_ANCHOR=$'        check_file_header(&header, node_id)?;\n'
python3 -I - "$ROOT/crates/raft-storage/src/lib.rs" "$OLD_ANCHOR" <<'PY'
import sys
text = open(sys.argv[1]).read()
print(f"   S1 anchor of wal_ignore_file_header occurs {text.count(sys.argv[2])} times in the current source")
PY

say "RESULT"
echo "2a and 3 are deterministic: the old rule says SURVIVED for a test that failed and for one that never ran;"
echo "the current rule says 'detected' and 'build_error'. 2b depends on timing and is shown only for comparison."
