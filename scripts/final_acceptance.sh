#!/bin/bash
# Full acceptance from scratch: mutation tests, simulation, both benchmarks, then every phase of
# acceptance/final.json, and finally scripts/status.py. (S6 ran the same steps from a local copy.)
#
#   bash scripts/final_acceptance.sh                      # writes to artifacts/final-acceptance/
#   OUT=artifacts/another-run bash scripts/final_acceptance.sh
#
# Resumable: every finished step leaves a marker in $OUT/done/, so a run that is interrupted
# (session end, WSL restart) continues where it stopped when started again with the same OUT.
# The source fingerprint is recorded at the first start; if a fingerprinted file changes
# afterwards the script stops, because every report and receipt would then describe another
# source. Start over with a new OUT in that case. It took about 45 minutes on one WSL machine.
set -u
ROOT="$(cd -- "$(dirname -- "$0")/.." && pwd)"
OUT="${OUT:-artifacts/final-acceptance}"
case "$OUT" in /*) ;; *) OUT="$ROOT/$OUT" ;; esac
PY="$ROOT/.venv/bin/python"
export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p "$OUT/done"
cd "$ROOT" || exit 1

fingerprint() {
  "$PY" -I -c "
import sys; sys.path.insert(0, 'scripts')
from pathlib import Path
from verify_gate import snapshot
print(snapshot(Path('.').resolve())[1])"
}
current=$(fingerprint) || exit 1
if [ -f "$OUT/fingerprint.txt" ]; then
  if [ "$(cat "$OUT/fingerprint.txt")" != "$current" ]; then
    echo "source changed since the first start ($(cat "$OUT/fingerprint.txt") -> $current); start over with a new OUT" | tee -a "$OUT/progress.log"
    exit 3
  fi
else
  echo "$current" > "$OUT/fingerprint.txt"
fi

log() { echo "$(date -Is)  $*  load=$(cut -d' ' -f1 /proc/loadavg)" | tee -a "$OUT/progress.log"; }
step() { # name, command...
  local name=$1; shift
  if [ -f "$OUT/done/$name" ]; then log "skip $name (done)"; return 0; fi
  log "start $name"
  "$@" > "$OUT/$name.log" 2>&1
  local code=$?
  echo "exit $code" >> "$OUT/$name.log"
  if [ $code -ne 0 ]; then log "FAILED $name (exit $code)"; tail -25 "$OUT/$name.log" | tee -a "$OUT/progress.log"; exit $code; fi
  touch "$OUT/done/$name"; log "done $name"
}

log "fingerprint $current"
step mutation "$PY" scripts/mutation_check.py
step simulation "$PY" scripts/simulation_report.py
step read_benchmark "$PY" scripts/read_benchmark.py
step write_benchmark "$PY" scripts/write_benchmark.py
for n in $("$PY" -I -c "import json;print(' '.join(sorted(json.load(open('acceptance/final.json'))['phases'])))"); do
  step "phase_$n" "$PY" scripts/verify_gate.py "$n"
done
"$PY" scripts/status.py | tee "$OUT/status.txt"
touch "$OUT/ALL_DONE"
log "ALL_DONE"
