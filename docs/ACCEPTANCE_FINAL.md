# 最終驗收契約

契約檔是 [acceptance/final.json](../acceptance/final.json)，由 `scripts/verify_gate.py` 執行。
它接替 [替代驗收 v1](ACCEPTANCE_V1.md)；v1 的契約檔與 receipt 保留為歷史紀錄。
和 v1 一樣，這是本專案自訂的有限工程驗收，不是遺失的原始 gate，也不是形式化證明。

## 與 v1 的關係

- 建立時（2026-11-12）階段 00～08 的內容與 v1 完全相同，另外加上一條共同命令：
  `python3 -B scripts/check_lessons.py`（檢查 bug 教學的結構）。
- 之後每個路線圖階段在規格獲准後，各自追加一個階段（預定 10～15，見契約的 `planned_phases`）。
- 後面的階段可以**取代**前面階段的測試（例如 S1 會以新儲存格式的測試取代 v1 的儲存測試），
  條件是更新契約裡的 `v1_mapping`：v1 的每一項測試都必須指向至少一個仍在契約中的測試。
  `scripts/test_verify_gate.py` 會檢查這張對照表完整且指向存在的測試。
  「新測試沒有比較弱」無法由腳本判斷，由每階段的獨立審查負責，並寫在對照表的 `note`。

## 來源指紋

checker 對專案內所有檔案計算雜湊，排除 `.git`、`.agf`、`.venv`、`target`、`artifacts`、快取目錄，
以及 **`.claude`、`.agents`**（2026-11-12 使用者決定）。後兩者是 AI 框架管理的宿主設定副本，
不影響程式行為，是否被改動由 `agf project status` 另行檢查。`docs/`、`scripts/`、`acceptance/`、
`crates/`、`tests/`、`tools/` 都在指紋內：改任何一個，既有 receipt 就不再適用於目前來源。

checker 或契約檔本身改變時，必須從第一個階段重新依序驗收。

## 操作

```bash
. "$HOME/.cargo/env"
.venv/bin/python scripts/status.py            # 各階段最近一次結果，以及是否對應目前來源
.venv/bin/python scripts/mutation_check.py    # 階段 10 起需要：產生對應目前來源的 mutation 報告
.venv/bin/python scripts/revalidate_all.py    # 依序重跑全部階段，寫入 artifacts/
bash scripts/run_phase.sh 03                  # 只跑到某一階段（需前一階段已有通過紀錄）
```

階段 10 要求 `artifacts/reports/mutations.json` 的 `source_fingerprint` 等於受驗收的來源，
並且列出的每個植入錯誤都被抓到。來源改過之後，先重跑 `mutation_check.py`（數分鐘），再跑驗收；
否則 P10 會以「報告不是由目前來源產生」失敗。這是刻意的：舊的 mutation 結果不能替新的程式作證。

receipt 位於 `artifacts/acceptance-final/`：`phase-XX.last-attempt.json` 是最近一次嘗試，
`phase-XX.json` 只在通過時更新。判斷現況請看前者與 `status.py` 的 `current source` 標示。

## 這份驗收不能說明什麼

契約的 `limits` 欄位列出完整清單。重點：具名測試被找到並執行通過，不代表斷言寫得夠強；
子程序測試涵蓋 process 被終止，不涵蓋主機斷電或儲存裝置故障；通過驗收不等於適合正式環境使用。
