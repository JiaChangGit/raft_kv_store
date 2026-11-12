#!/usr/bin/env bash
# Lesson 03: turn on log compaction before InstallSnapshot exists, and the first
# leader that needs a discarded entry has nothing it can send.
# Builds two copies of the current code under one temporary directory, removed at exit.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
SEEDS="${SEEDS:-60}"
EVENTS="${EVENTS:-3000}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/compaction-order.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

say() { printf '\n== %s\n' "$*"; }
campaign() {
  (cd "$1" && CARGO_TARGET_DIR="$WORK/target-$2" cargo run -q --release --locked -p raft-sim -- \
     --seeds "$SEEDS" --events "$EVENTS" --output "$WORK/$2.json")
}

say "1. Two copies of the current code"
for copy in without with; do
  mkdir "$WORK/$copy"
  git -C "$ROOT" archive HEAD | tar -x -C "$WORK/$copy"
done
CORE="$WORK/without/crates/raft-core/src/lib.rs"
ANCHOR='            return self.send_snapshot_chunk(peer, effects);'
[ "$(grep -cF "$ANCHOR" "$CORE")" -eq 1 ] || { echo "anchor not found"; exit 1; }
# Put back what step 1 of S2 had here, before InstallSnapshot existed.
sed -i "s|$ANCHOR|            unreachable!(\"log compaction is not enabled yet; entry {prev} was discarded\")|" "$CORE"
echo "   'without': the leader has no way to send a snapshot (as after S2 step 4)"
echo "   'with':    unchanged"

say "2. Simulation with compaction on, $SEEDS seeds x $EVENTS events, WITHOUT InstallSnapshot"
if campaign "$WORK/without" without >"$WORK/without.log" 2>&1; then
  echo "NOT REPRODUCED: the campaign passed"; exit 1
fi
grep -m1 -E "panicked|unreachable" "$WORK/without.log" | sed 's/^/   /'
grep -m1 -E "entered unreachable code" "$WORK/without.log" | sed 's/^.*code: /   reason: /' || true
echo "BUG REPRODUCED: a leader needed an entry it had already compacted away."

say "3. The same campaign WITH InstallSnapshot"
campaign "$WORK/with" with >"$WORK/with.log" 2>&1 || { echo "UNEXPECTED: failed"; tail -5 "$WORK/with.log"; exit 1; }
python3 -I - "$WORK/with.json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
print(f"   passed {r['pass_count']}/{r['seed_count']} seeds, {r['event_count']} events")
print(f"   snapshots taken {r['snapshots_taken']}, installed through InstallSnapshot {r['snapshots_installed']}")
print(f"   invariants checked: {', '.join(i.split()[0] for i in r['invariants'])}")
PY

say "RESULT"
echo "Compaction is only safe together with a way to send the discarded prefix."
