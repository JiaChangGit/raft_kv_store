# 測試紀錄：S1～S4 獨立審查與修正（2026-11-14）

這份文件記下 2026-11-14 實際執行過的測試：命令、結果、證據放在哪裡，以及哪些事**沒有**做。
背景與發現的內容見 [獨立審查紀錄](INDEPENDENT_REVIEW_S1_S4.md)。

讀法：

- 「已執行」表示命令在這台機器上實際跑過，數字取自輸出或報告檔。「推論」與「未執行」另外標明。
- 時間是 UTC，取自報告或 receipt 內的時間戳；沒有時間戳的只寫日期（台灣時間 2026-11-14 凌晨到上午）。
- `artifacts/` 不在版本控制內，也不在來源指紋內。下面提到的 `artifacts/...` 路徑只存在這台機器上。
- **來源指紋**是 `scripts/verify_gate.py` 對 `crates/`、`tests/`、`tools/`、`scripts/`、`acceptance/`、`docs/` 等檔案算出的雜湊；改任何一個檔案，舊的 receipt 就不再代表目前的來源。

## 1. 環境

| 項目 | 值 |
|---|---|
| 機器 | WSL2，`Linux-6.18.33.2-microsoft-standard-WSL2-x86_64`，16 個 CPU |
| Rust | `rustc 1.99.1 (58cf8f9c3 2026-10-13)`（`rust-toolchain.toml` 指定） |
| Python | 3.14.4（專案的 `.venv`） |
| 審查對象 | commit `513cd58`（tag `s4-membership-verified`） |
| 修正後的來源 | 同一個 commit 加上未提交的工作樹變更；§5 的結果對應指紋 `5209bbdf…390dc` |

## 2. 審查開始時的基準（`513cd58`，工作樹乾淨）

| 執行了什麼 | 結果 |
|---|---|
| `cargo test --workspace` | 110 個通過，0 失敗 |
| `.venv/bin/python scripts/status.py` | P00～P13 皆 `PASS [current source]`（2026-11-13 的 receipt） |

## 3. 審查期間的實驗（都在暫存副本上，沒有改動來源）

重現程式在 `artifacts/audit/independent-review-20261114/repro/`，輸出在同一資料夾的 `evidence/`。

| # | 實驗 | 結果 | 證據 |
|---|---|---|---|
| R1 | 目前的模擬器＋植入「不等多數確認就回覆讀取」，1,000 種子 × 10,000 事件 | 全部通過（沒有抓到） | `evidence/sim_vs_mutants.json` |
| R2 | 同一個植入編進 `raft-node`，跑 `test_faults.py`、`test_read_index.py`、`test_randomized.py` | 6 個全部通過（沒有抓到） | 只有終端輸出，未存檔 |
| R3 | 模擬器改成把請求送給 term 最低的自認 leader，正式的 core | 1,000 種子全過，讀取 19,278 次 | `evidence/sim-stale-prod.json` |
| R4 | R3 的模擬器＋R1 的植入 | 第 260 個種子失敗：`I8 Read Freshness … readIndex=14 < committed 15` | `evidence/sim-stale-mutant.json` |
| R5 | 29 個位於 raft-core、raft-protocol 的植入錯誤，各自**只**跑模擬器 | 抓到 12 個，17 個存活 | `evidence/sim_vs_mutants.json`，腳本 `repro/sim_vs_mutants.py` |
| R6 | R5 中與讀取有關的 6 個，改用 R3 的模擬器 | 抓到 5 個 | `evidence/sim_stale_vs_mutants.json` |
| R7 | 正式的 `Core`＋`Wal`：term 3 的節點以一則訊息安裝 term 5 的快照，模擬在兩個檔案之間被終止 | 兩次 `Wal::open` 都回 `snapshot is newer than the durable term` | `repro/review_install_newer_term.rs` |
| R8 | 真實 process：三台、`--snapshot-every 3`，follower 停機跨越兩次換 leader，在中斷點 `install_after_snapshot_before_wal` 被 `SIGKILL` | 之後兩次啟動都以 exit code 1 結束，訊息同 R7；叢集其餘兩台照常服務 | `repro/repro_install_newer_term.py`，`evidence/repro-install-node-2.stderr` |
| R9 | 模擬器只把快照分塊從 7 改成 4096 bytes | 第 249 個種子失敗：`invalid identity or durable state`（節點 2 的 term 89，快照 term 94） | `evidence/sim-chunk4096.json` |
| R10 | R9 再加上「回復時採用快照的 term」（只改模擬器的磁碟模型） | 1,000 種子全過，安裝快照 16,642 次 | `evidence/sim-chunk4096-termfix.json` |
| R11 | 67,473,925 bytes 的狀態機，量 `StateMachine::snapshot()` | 一次約 90～100 ms（當時同時有其他建置在跑；§6 重量是 50 ms） | `repro/review_snapshot_cost.rs` |
| R12 | 以腳本比對 27 個 v1 測試在 V1 tag 與 `513cd58` 的函式內容 | 12 個相同、1 個被取代、14 個有修改（都已記錄或為機械性修改） | 只有終端輸出；結論在審查紀錄 §7 |

**作廢的一批**：R5 的第一次執行讓所有案例共用一個 cargo 建置目錄，前一個植入殘留到後面（教學 06 記過的錯誤），
徵兆是互不相干的案例都回報 `committed conflict index=57`。那批結果沒有採用，也沒有保留。上表是修正腳本後重跑的結果。

## 4. 修正：先看到失敗，再看到通過

| 測試 | 修正前 | 修正後 |
|---|---|---|
| `interrupted_install_from_a_newer_term_raises_the_term`（新增，raft-storage） | 失敗：`a node must start after this interruption: snapshot is newer than the durable term` | 通過 |
| `real_process_crash_during_install_from_a_newer_term_recovers`（新增，真實 process） | 失敗於 `install_after_snapshot_before_wal`，訊息相同 | 通過（四個中斷點） |
| `snapshot_and_wal_consistency_rules`（既有） | 修正程式後失敗：它的最後一列斷言這個狀態「必須拒絕」 | 把那一列改成「採用快照的 term」後通過；已記入契約的 `modified_tests` |
| `an_oversize_state_is_not_serialized_again_until_the_log_doubles`（新增，raft-node） | 以 mutation 案例確認：拿掉修正時 `left: 59, right: 9` | 通過 |
| `test_a_leader_left_with_a_minority_does_not_answer_reads`（新增，五台真實 process） | 把 R1 的植入編進節點時失敗：舊 leader 回覆 `{'status': 'OK', 'result': 'old', …}` | 正式的程式連跑 3 次都通過（每次約 2.4 秒） |
| R8 的真實叢集腳本 | 兩次啟動都被拒絕 | 兩次都啟動：term 4、快照位置 24，並追上其他節點（`evidence/repro-install-after-fix.txt`） |

修正前的失敗輸出存在 `artifacts/audit/independent-review-20261114/evidence/f1-red-before-fix.txt`。

模擬器修改過程中另外量到的三件事（都已執行）：

- 只做「隨機挑任一自認的 leader」：1,000 種子裡 57 次請求送到過期 leader。加上「兩個 leader 並存時主動對舊的發讀取」後是 2,502 次。
- 只做隨機挑選並植入 R1 的錯誤：1,000 種子全過，送到過期 leader 43 次。所以主動發讀取是必要的（教學 12 的練習）。
- 分塊改為隨種子變化之後，把磁碟模型的修正拿掉：1,000 種子全過。把 follower 安裝途中 crash 的機率從 1/100 調到 1/10 後才抓得到。

## 5. 修正後的完整執行（指紋 `5209bbdfab1b0265e2c9d29e5f33d5a1c270b94e134cf95ac3d6d9b3297390dc`）

這一節的每一項都對應同一份來源：commit `513cd58` 加上修正（不含本文件與這次改寫的交接文件）。

| 執行了什麼 | 時間（UTC） | 結果 | 報告或紀錄 |
|---|---|---|---|
| `cargo fmt --all -- --check` | 2026-11-14 | 通過 | 驗收各階段的共同命令 |
| `cargo clippy --workspace --all-targets -- -D warnings` | 2026-11-14 | 通過 | 同上 |
| `cargo test --workspace` | 03:05～03:06 | 113 個通過，0 失敗，0 忽略 | 終端輸出 |
| `.venv/bin/python -B -m pytest -q -p no:cacheprovider tests scripts` | 03:04～03:05 | 84 個通過，57 秒 | 終端輸出；各測試的節點紀錄在 `artifacts/runs/` |
| `python3 -B scripts/check_lessons.py` | 2026-11-14 | 13 篇，0 個問題 | 終端輸出 |
| `.venv/bin/python scripts/mutation_check.py` | 02:24 起 | 49 個植入全部被抓到；40 個對照組全部通過；每個案例獨立建置 | `artifacts/reports/mutations.json`，各案例在 `artifacts/mutations/20261114T022436Z/` |
| `.venv/bin/python scripts/simulation_report.py` | mutation 之後 | 1,000 種子 × 10,000 事件全部通過（I1～I10） | `artifacts/reports/simulation.json` |
| `.venv/bin/python scripts/read_benchmark.py` | 02:35 | 300 次 GET：S2 版 p50 2.23 ms、p99 3.45 ms、每台寫 300 筆 WAL record；目前版本 p50 1.83 ms、p99 2.93 ms、0 筆 | `artifacts/reports/read-benchmark.json`（只記錄，不是門檻） |
| `.venv/bin/python scripts/revalidate_all.py` | 02:35:57～02:57:15 | P00～P13 共 13 個階段全部 `PASS` | `artifacts/revalidation/20261114T023557Z/summary.json`，receipt 在 `artifacts/acceptance-final/` |

模擬報告的統計（修正後的工作負載）：

| 項目 | 數值 | 項目 | 數值 |
|---|---|---|---|
| 選舉 | 46,301 | 套用 | 390,071 |
| 做快照 | 77,735 | 安裝快照 | 13,740 |
| 回覆的讀取 | 19,271 | 失敗的讀取 | 5,023 |
| 完成的成員變更 | 17,722 | leader 移除自己 | 2,545 |
| 追趕失敗 | 49 | 偶數大小的設定 | 9,645 |
| **送到過期 leader 的請求** | 2,502 | **一則訊息送完的快照** | 7,024 |

最後兩項是這次新增的；測試斷言它們大於零（教學 11、12）。失敗的讀取比修正前多（修正前 2,575），是因為現在會對過期的 leader 發讀取，而它們應該失敗。

驗收契約 `acceptance/final.json` 目前具名列出 102 個 Rust 測試與 30 個 pytest 節點，各階段的數量：

| 階段 | Rust | pytest | 階段 | Rust | pytest |
|---|---|---|---|---|---|
| 00 基礎 | 3 | 0 | 07 檢查器與隨機驗證 | 1 | 3 |
| 01 選舉 | 3 | 0 | 08 benchmark | 0 | 1 |
| 02 複製 | 3 | 0 | 10 S1 儲存格式 | 20 | 1 |
| 03 WAL | 4 | 0 | 11 S2 快照與壓縮 | 33 | 6 |
| 04 TCP 與真實 process | 2 | 1 | 12 S3 ReadIndex | 9 | 5 |
| 05 KV 與重試 | 3 | 1 | 13 S4 成員變更 | 21 | 9 |
| 06 故障注入 | 0 | 3 | | | |

這次加進契約的：階段 11 多 3 個 Rust 測試，階段 12 多 1 個 pytest 節點；mutation 的必抓案例多 4 個；模擬報告多 2 個必須大於零的欄位。

## 6. 三篇教學的重現腳本（都已執行，輸出存在各篇的 `expected-output.txt`）

| 腳本 | 結果 |
|---|---|
| `docs/lessons/11-install-from-a-newer-term/repro.sh` | `s4-membership-verified`：重啟兩次都 `REFUSED`；目前來源：兩次都 `STARTED with term 5 vote None` |
| `docs/lessons/12-simulator-stopped-asking-stale-leaders/repro.sh` | 植入同一個錯誤：S3 的模擬器在第 266 個種子抓到；S4 的 1,000 種子全過；目前來源在第 21 個種子抓到 |
| `docs/lessons/13-oversize-snapshot-serialized-every-event/repro.sh` | 67 MiB 的狀態機、10 個事件：修正前每次 50 ms 共 500 ms；修正後 52 ms、序列化 1 次 |

教學的練習題另外各跑了一次，結果寫在各篇的 `answers.md`。

## 7. 沒有執行的，以及推論

- **沒有用真實叢集跑到 64 MiB。** 「超過上限後節點會越來越慢並引發改選」是由每個事件的成本推論的。
- **沒有測主機斷電。** 所有中斷測試都是 process 被 `SIGKILL`，沿用 S1、S2 已聲明的限制。
- F1 的四個觸發條件中，真實 process 只重現了「停機跨越兩次換 leader」；「被網路隔離後回來」是由程式推論的。
- 審查紀錄的觀察 O1（每個事件複製整份 log 的成本）與 O3（單一封包遺失約 19% 機率引發改選）是估算，沒有量測。
- 沒有重跑 V1 時期的 `scripts/clean_rebuild.py`（乾淨副本重建）；那屬於 S6。
- 教學 11～13 沒有第二個人照著做過，狀態都是「草稿」。
- S1 的審查者也參與過 S1，S1 沒有經過完全獨立的審查。

## 8. 這份文件加入之後

加入本文件會改變來源指紋，§5 的 receipt 因此不再對應目前的來源。寫完後依下面的順序重跑了一次；
結果不寫在這裡（寫了又會再改一次指紋），請以 `scripts/status.py` 為準：每一行都是 `PASS [current source]` 就代表那次重跑對應目前的檔案。

## 9. 怎麼重跑

```bash
cd /home/user/src/Distributed_Key-Value_Store_with_Raft
. "$HOME/.cargo/env"                             # 非登入的 shell 裡 cargo 不在 PATH 上
.venv/bin/python scripts/status.py               # 先看現況
.venv/bin/python scripts/mutation_check.py       # 約 10 分鐘；報告帶著來源指紋
.venv/bin/python scripts/simulation_report.py    # 約 1 分鐘
.venv/bin/python scripts/read_benchmark.py       # 約 1 分鐘；會建置 S2 tag 的節點程式
.venv/bin/python scripts/revalidate_all.py       # 約 21 分鐘，P00～P13
```

三份報告都記著產生它們的來源指紋，驗收只接受指紋相同的報告。所以**從第一個命令開始到最後一個結束，不要改動任何受指紋涵蓋的檔案**（包含 `docs/`）。
只想確認某個階段自己的測試時，用 `.venv/bin/python scripts/preflight_phase.py 11`（幾分鐘，不寫 receipt）。
