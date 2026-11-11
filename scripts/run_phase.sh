#!/usr/bin/env bash
# Alternative acceptance wrapper only. This does not invoke an AI or generate code.
set -euo pipefail
cd -- "$(dirname -- "$0")/.."
if [[ $# != 1 || ! "$1" =~ ^0[0-8]$ ]]; then
  printf 'Usage: bash scripts/run_phase.sh 00..08\n' >&2
  exit 2
fi
if [[ -x .venv/bin/python ]]; then
  exec .venv/bin/python scripts/verify_gate.py "$1"
fi
exec python3 scripts/verify_gate.py "$1"
