#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")/.."
command -v cargo rustc python3
rustc -Vv
cargo -V
rustup component list --installed
python3 --version
python3 -m pytest --version
python3 - <<'PY'
import json, platform, shutil
from pathlib import Path
print(json.dumps({'platform':platform.platform(),'root':str(Path.cwd()),'disk_free_bytes':shutil.disk_usage('.').free,'meaning':'Tool execution only; no authentication, model, durability, or protocol validation.'},indent=2))
PY
