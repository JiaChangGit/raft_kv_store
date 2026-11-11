#!/usr/bin/env bash
# Final acceptance wrapper only. This does not invoke an AI or generate code.
set -euo pipefail
cd -- "$(dirname -- "$0")/.."
if [[ $# != 1 ]]; then
  printf 'Usage: bash scripts/run_phase.sh PHASE (a phase key in acceptance/final.json)\n' >&2
  exit 2
fi
if [[ -x .venv/bin/python ]]; then
  exec .venv/bin/python scripts/verify_gate.py "$1"
fi
exec python3 scripts/verify_gate.py "$1"
