# Rust Raft KV Store

從零實作的三節點、單一 Raft group 教學與工程實驗專案。固定 IDs 1/2/3，quorum=2，
三個獨立 Linux process；同一份 `raft-core` 用於虛擬模擬器與 TCP runtime。

原始套件、gates.json、runner/checker 未提供。三份原始 Markdown 完整保留，
本專案採用另行定義的驗收契約，不宣稱等同原 gate 或形式化證明。目前有效的是
[最終驗收契約](docs/ACCEPTANCE_FINAL.md)（`acceptance/final.json`）；較早的
[替代驗收 v1](docs/ACCEPTANCE_V1.md) 保留為歷史紀錄。目前結果請執行
`.venv/bin/python scripts/status.py`，並以 `artifacts/acceptance-final/` 內的 receipt 為準；
程式存在不代表所有驗收已完成。後續規劃見 [路線圖](docs/ROADMAP.md)，開發中遇到的 bug 見 [Bug 教學](docs/lessons/README.md)。

## 環境與建置

在 WSL2 Linux home/ext4 執行，不用 `/mnt/c` 作 WAL 或效能結論。已安裝的工具
版本與範圍見 `artifacts/reports/environment.json`。Rust 使用 rustup 與固定版本
`rust-toolchain.toml`，Python 依賴僅安裝在 `.venv`，不需要 sudo 執行本專案。

```bash
cd /home/user/src/Distributed_Key-Value_Store_with_Raft
. "$HOME/.cargo/env"
. .venv/bin/activate
bash scripts/preflight.sh
cargo build --workspace --locked
```

重建 Python 環境時：`python3 -m venv .venv`，再執行
`.venv/bin/python -m pip install -r requirements-dev.txt`。這些命令會下載依賴／寫檔。

## 手動啟動與 client

三個 terminal 分別執行（各 terminal 先載入 cargo env）：

```bash
cargo run -p raft-node -- --id 1 --config config/cluster.toml --data-dir data/node-1
cargo run -p raft-node -- --id 2 --config config/cluster.toml --data-dir data/node-2
cargo run -p raft-node -- --id 3 --config config/cluster.toml --data-dir data/node-3
```

同一資料目錄不能由兩個 process 共用，也不能交換 node identity。另一個 terminal：

```bash
.venv/bin/python -m tools.kv_client --address 127.0.0.1:7201 --address 127.0.0.1:7202 --address 127.0.0.1:7203 --client-id 7 --sequence 0 Put x A
.venv/bin/python -m tools.kv_client --address 127.0.0.1:7201 --address 127.0.0.1:7202 --address 127.0.0.1:7203 --client-id 7 --sequence 1 Get x
```

每個 client 同時只有一筆 logical request。TIMEOUT_UNKNOWN 使用相同 ID/sequence/content
重試；新請求增加 sequence。Put/Delete 回傳先前值，Get 回傳當時值。
所有新 GET 都進 Raft log；debug status 的 KV map 不提供線性化讀取契約。
停止節點可用 Ctrl-C；故障測試另外使用 SIGKILL，並保留原資料目錄重啟。

重複執行上面的固定 client ID／sequence 時，既有資料目錄會保留 session，
因此是重試舊請求，不是新操作。要完整體驗隔離的三程序啟動、PUT／GET、
SIGKILL 與原目錄重啟，可依 [實際操作帶讀](docs/HANDS_ON.md) 使用新的資料目錄。

## 驗證

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --bins
.venv/bin/python -m pytest -q tests
.venv/bin/python scripts/verify_gate.py 00
```

後續依序執行 `01` 到 `08`。每次 gate 累積先前必測、精確 discovery、逐項實跑、
完整 Rust 回歸與對應 process tests；失敗保留原始輸出，不改條件繞過。
`scripts/run_phase.sh XX` 只是單階段驗收 wrapper，不呼叫模型或自動產生程式。
測試 profile 僅優化執行速度，隨機測試預設仍是 1000 seeds × 10000 events。

```bash
cargo run --release -p raft-sim -- --seeds 1000 --events 10000 --output artifacts/reports/verification.json
.venv/bin/python -m tools.benchmark --directory artifacts/runs/benchmark-manual --output artifacts/reports/benchmark.json --samples 12
.venv/bin/python scripts/mutation_check.py
```

Mutation 只修改 `artifacts/mutations/` 隔離副本，展示錯誤 quorum、舊 term 提交及
反向排程會被測試抓到，不修改正式程式或 gate。process tests 的 PID、WAL、history、
fault schedule 與 logs 留在 `artifacts/runs/`；這些本機資料預設不納入 Git。

## 設計與帶讀

| 模組 | 責任 |
|---|---|
| raft-protocol | 強型別 RPC、ID、尺寸與格式限制 |
| raft-core | 純 Event → 有序 Effect，共識狀態單一擁有者 |
| raft-sim | 虛擬時計、MockDisk、故障排程、獨立 I1～I5 observer |
| raft-storage | checksummed append-only WAL transaction、sync、recovery |
| kv-state | deterministic KV、完整 replicated session records |
| raft-transport | 有界 frame 讀寫與 RPC correlation |
| raft-node | actor、同步 storage barrier、TCP、timers、client reply |

推薦讀 [API 契約](docs/API_CONTRACT.md)、[commit 規則](docs/COMMIT_RULE_REVIEW.md)、
[WAL 格式 2](docs/STORAGE_FORMAT_V2.md)、[client 契約](docs/CLIENT_CONTRACT.md)、
[故障排程](docs/FAULT_SCHEDULE_FORMAT.md)、[帶讀與練習](docs/LEARNING_GUIDE.md)。
Raft 原始權威來源為 [extended paper](https://raft.github.io/raft.pdf) Figure 2/3、§5.4；
本專案規格與論文的差異必須明示。

## 框架與限制

使用相鄰 `ai-general-framework` 的 shared tools 與五個按需 skills；該 checkout 維持唯讀。
`.agf/managed/project-rules.md` 指向有效入口；沒有假設宿主已自動載入或框架會永久記憶。
經驗與限制見 [框架回饋](docs/FRAMEWORK_FEEDBACK.md)。跨對話接手讀
[WORKLOG](docs/WORKLOG.md)，再核對目前檔案與實際結果。
獨立審查發現、負向回歸與乾淨副本的操作方式見[工程強化紀錄](docs/ENGINEERING_HARDENING.md)。

無 snapshot/compaction、session GC、動態成員、TLS、sharding 或 Multi-Raft。
WAL/session 持續增長；本專案用於有限規模實驗，不作長期 production database。
非 Byzantine 模型；假設成功的 sync_all 由 OS/磁碟正確履行，SIGKILL 不等於硬體斷電。
安全性與進展分開：永久失去多數時不保證可用。效能數字只代表該次單機 WSL 環境。
