# 06. Environment & Operations

## 1. Windows 11 + WSL2 Ubuntu

- 將 repo 與 WAL 放在 `~/projects/...` (WSL Linux filesystem)，不要用 `/mnt/c` 做 durability 或 latency 結論。
- 必須確認 `rustc`, `cargo`, `rustfmt`, `clippy`, `python3`, `pytest`, `git`, `codex` 在 **WSL Ubuntu shell** 可執行。
- 先完成 `codex login`。如 Cargo dependency fetch 受 Codex sandbox 網路限制，**使用者可在外部預先 `cargo fetch`**，而不是啟用危險的 `--yolo`。
- 每個 Process 採單獨 `data/node-{1,2,3}`；不要使用同一 WAL 目錄。

## 2. Initial runtime settings

```
peer ports:       7101 / 7102 / 7103 (via proxy endpoints in fault tests)
client ports:     7201 / 7202 / 7203
heartbeat:        100 ms
election:         uniform random 400–800 ms
rpc timeout:      250 ms
client timeout:   3000 ms
append batch:     32 entries
frame limit:      1 MiB
key/value bounds: 256 / 4096 bytes
```

這是初始測試參數，不是 timing guarantee。Time 對 Safety 不應有影響；Liveness 與性能需基於實際 trace 調整。

## 3. Tool Commands

```
bash scripts/preflight.sh
bash scripts/run_phase.sh 00
python3 scripts/verify_gate.py 00
bash scripts/run_phase.sh 01
# ...
```

Codex CLI 一階段執行以 `codex exec --sandbox workspace-write --cd <repo> -` 從 stdin 讀取 phase prompt。執行產生的輸出保存在 `artifacts/codex/`。若未安裝 Codex，僅能閱讀 spec，不等於已執行。

## 4. Fault Proxy

Proxy 需按 A->B 與 B->A 不同方向處理**完整 frame**：allow/drop/delay/duplicate/reorder，並能切斷現存連線，否則測試中的隔離可能不完整。Packet-bytes 在 TCP stream 裡面不保證 frame boundary；proxy 需先解析 framing。

## 5. SIGKILL Crash and Restart

Python subprocess 監控 Process PID、stdout/stderr、退出代碼。Crash 使用 `SIGKILL` 而非 graceful shutdown；Restart 使用**原資料目錄**。每次可留 Node WAL dump、RPC trace、Client history。

## 6. Reporting and Maintenance

每天或每次 Phase 完成，保存 Git checkpoint；變更協定/格式須更新 ADR、paper mapping 及 tests。WAL 無 compaction，需設定測試上限、磁碟空間監控。**不能**為了加快性能跳過 fsync 或回覆前屏障。
