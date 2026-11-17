# 替代驗收 v1

> **2026-11-12 起為歷史紀錄。** 目前有效的契約是 [最終驗收契約](ACCEPTANCE_FINAL.md)；本文件與 `acceptance/v1.json`、
> `artifacts/acceptance-v1/` 內的 receipt 保留原樣，供對照當時的條件與證據。下文的命令對應當時的 checker，現在的腳本已改讀最終契約。

本專案只收到三份 Markdown，未收到原始 ZIP、`gates.json`、runner 或 checker。`docs/reference/` 是從 `ALL_IN_ONE_SPEC.md` 抽出的便利閱讀副本，不是找回的原始套件；來源標示、SHA-256 與缺少資產見 [SOURCE_MANIFEST.json](SOURCE_MANIFEST.json)。根目錄三份原始材料保留。

使用者已同意建立完整 V1、自行準備環境並採用另訂的替代驗收。正式的本地驗收契約是 [acceptance/v1.json](../acceptance/v1.json)，名稱固定為 `alternative-acceptance-v1`。它取用彙整規格的具名測試、階段順序與報告要求，但不能宣稱與未取得的原版等價。

## 操作

在 WSL 的專案根目錄、Rust 與專案虛擬環境已準備好後執行：

```bash
export PATH="$HOME/.cargo/bin:$PATH"
.venv/bin/python scripts/verify_gate.py 00
bash scripts/run_phase.sh 01
```

每次只指定一個階段，依序到 `08`。`run_phase.sh` 只選擇專案 Python 並呼叫替代 checker，**只執行檢查，不會產生程式、不會自動執行後續階段**。這與原始材料所描述、但未提供的同名 runner 不同。

`preflight.sh` 查工具實際版本與 pytest 可執行性。若用虛擬環境，可先將 `.venv/bin` 加入 PATH；它不安裝、不登入，不把發現執行檔當功能成功。建置、測試與驗收會寫 `target/`、cache、測試資料及 `artifacts/`。

## 機械驗收

每個階段累積檢查 P00 到指定階段：

1. 必要檔案存在且非空。
2. `cargo test --workspace -- --list` 的實際輸出中，每個要求的 Rust leaf name 恰好對應一個完整測試名稱。名稱不存在或重複即失敗。
3. `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、完整 workspace tests 全部成功。
4. 再以發現到的完整名稱逐一 `--exact` 執行；核對該測試明確 `ok`，總計一項通過、零失敗、零忽略。其他 harness 的零項測試不算成功證據。
5. pytest 先收集指定完整 node ID，再逐項執行；以 JUnit 確認真正執行一個沒有 skip/error/failure 的 testcase。退出碼 0 不能掩蓋空跑。
6. P07/P08 報告具備要求欄位；數值來自實跑，原始資料另供人工追溯。只有 schema 有效不代表數字真實。
7. 驗收前後來源 fingerprint 相同；驗收期間有其他代理改檔、Cargo 初次產生 lockfile或其他來源變動時，保留這次診斷，待穩定後重跑。

所有命令使用參數陣列，逐項保存 stdout/stderr、SHA、退出碼、耗時及是否因逾時而擷取不完整。每個 run 有獨立目錄，先前失敗不被覆寫。

## Receipt 與版本

證據位於 `artifacts/acceptance-v1/PXX-<UTC時間>/`。`phase-XX.last-attempt.json` 是最近一次嘗試；`phase-XX.json` 只在成功時更新。保留的舊成功檔不代表目前最近嘗試通過。

P01 起要求前一階段最近嘗試成功，且契約與 checker 的 SHA 都相同。前階段記錄其當時 source fingerprint，後續階段可以增加或修改程式；本階段累積重跑前階段要求。這是本替代版本的明確語意，並非原始 fingerprint 規則。

修改契約或 checker 是新的驗收條件：必須記錄理由與差異，重新審查並從 P00 重驗，不可為讓已知錯誤程式通過而降低要求。原始提供文件、來源 manifest與工作規格的變更也應可追溯。

本次checker補強了pytest前建置實際process bins，以及核對三份原始材料hash；均增加檢查，
沒有移除或降低測試要求。完整新hash鏈從P00重跑。`revalidate_all.py`只針對已審查實作
依序重跑驗收，首敗停止，不跨階段自動產生程式。

## 人工審查與非保證

機械 checker 不會判斷 assertion 是否有意義、三個 PID 是否真的獨立、fault proxy 是否被繞過，或測量是否可重現；這些由相應測試內容與獨立人工／代理審查補足。必須核對測試觸及真實路徑、負向案例能抓錯、trace 與報告可回查實跑。

P03 在正式 runtime出現前可用真實 storage helper與 subprocess 測檔案／crash；完整 node 邊界在 P04/P06 重驗。P05 若需要隔離舊 Leader 的真實測試，最小 proxy設施是已記錄的前置依賴，不能先把未執行隔離列通過再到 P06補做。

必須分開：腳本退出成功、指定測試確實通過、有限情境未發現協定違規、形式化證明。這套驗收只提供有限工程證據，不聲稱形式化正確性、任意斷電保證或跨機效能。

