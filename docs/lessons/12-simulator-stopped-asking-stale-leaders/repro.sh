#!/usr/bin/env bash
# Lesson 12: plant the same mistake ("answer a read without waiting for a
# majority to confirm leadership") in three versions and let ONLY the simulator
# look for it. S3's simulator finds it; S4's does not, although nothing in the
# read path changed: S4 changed whom the simulated clients talk to.
# Builds three versions in a temporary directory; changes nothing in the repository.
set -euo pipefail

LESSON="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd -- "$LESSON/../../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/stale-leader-reads.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
say() { printf '\n== %s\n' "$*"; }
SEEDS=1000
EVENTS=10000
CORRECT='            Some(read_index) if read.round <= confirmed && last_applied >= read_index => {'
MISTAKE='            Some(read_index) if last_applied >= read_index => {'

campaign() {  # campaign NAME PLANT: run the simulator on $WORK/NAME, print one line
  local dir="$WORK/$1"
  if [ "$2" = yes ]; then
    python3 -I - "$dir/crates/raft-core/src/lib.rs" "$CORRECT" "$MISTAKE" <<'PY'
import sys
path, old, new = sys.argv[1:]
text = open(path).read()
assert text.count(old) == 1, "anchor not found"
open(path, "w").write(text.replace(old, new, 1))
PY
    # A copy keeps old modification times; make sure cargo rebuilds (lesson 06).
    touch "$dir/crates/raft-core/src/lib.rs"
  fi
  (cd "$dir" && CARGO_TARGET_DIR="$dir/target" cargo run -q --release --locked -p raft-sim -- \
     --seeds "$SEEDS" --events "$EVENTS" --output "$dir.$2.json" >/dev/null 2>"$dir.$2.err") || true
  python3 -I - "$dir.$2.json" <<'PY'
import json, sys
report = json.load(open(sys.argv[1]))
extra = ""
if "stale_leader_requests" in report:
    extra = f"; requests that reached a deposed leader: {report['stale_leader_requests']}"
if report["failures"]:
    failure = report["failures"][0]
    print(f"FAILED at seed {failure['seed']}: {failure['invariant']}{extra}")
else:
    print(f"all {report['pass_count']} seeds passed; reads answered: {report['reads_served']}{extra}")
PY
}

for ref in s3-read-index-verified s4-membership-verified current; do
  case $ref in
    s3-*) what="S3: requests go to the lowest-numbered node that believes it is leader" ;;
    s4-*) what="S4: requests go to the leader with the highest term" ;;
    current) what="current source: any self-believed leader, plus reads aimed at deposed ones" ;;
  esac
  say "$ref  ($what)"
  mkdir "$WORK/$ref"
  if [ "$ref" = current ]; then
    (cd "$ROOT" && git ls-files -co --exclude-standard -z | tar --null -T - -cf -) | tar -x -C "$WORK/$ref"
  else
    git -C "$ROOT" archive "$ref" | tar -x -C "$WORK/$ref"
  fi
  echo "   control (correct read path):      $(campaign "$ref" no)"
  echo "   mistake planted in raft-core:      $(campaign "$ref" yes)"
done

say "RESULT"
echo "The planted mistake is the same line in all three versions. Whether the simulator"
echo "sees it depends only on whether its clients ever talk to a deposed leader."
