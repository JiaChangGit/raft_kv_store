# 文件地圖

文件分成五類：**現行說明**照現在的程式寫；**規格與補記**是每個階段的規格與實作後的結果；**紀錄**寫的是當時發生的事，保留原文、不隨程式更新，裡面的路徑是作者機器上的路徑；**歷史**是已被取代的版本，開頭都有標示，指向現在以哪份為準；**原始材料**是專案開始時拿到的規格，不修改。

第一次讀，照這個順序：[從零開始](SETUP.md)、[架構說明](ARCHITECTURE.md)、[帶讀](LEARNING_GUIDE.md)、[bug 教學](lessons/README.md)。看不懂的名詞查 [名詞表](GLOSSARY.md)。

## 現行說明

| 文件 | 內容 |
|---|---|
| [SETUP.md](SETUP.md) | 從零開始：安裝工具、建置、跑測試、啟動三台的叢集、完整驗收、常見問題 |
| [ARCHITECTURE.md](ARCHITECTURE.md) | 元件、一次寫入與一次讀取的流程、成員變更；附 Mermaid 圖與對照的程式位置 |
| [LEARNING_GUIDE.md](LEARNING_GUIDE.md) | 帶讀：跟著一筆 PUT 走完整個系統，附練習 |
| [HANDS_ON.md](HANDS_ON.md) | 實際操作：一行命令跑完選舉、寫入、終止 leader、恢復，並解釋每個數字 |
| [lessons/](lessons/README.md) | 21 篇 bug 教學（草稿），每篇都有重現步驟與當時的輸出；索引裡有每個 bug 怎麼發現、怎麼找到根因的總表 |
| [FAQ.md](FAQ.md) | 自問自答：27 個問題檢查自己懂了沒有，每題附可以驗證答案的地方 |
| [GLOSSARY.md](GLOSSARY.md) | 名詞與縮寫 |
| [DEVELOPMENT.md](DEVELOPMENT.md) | 開發歷程：各階段做了什麼、為什麼是這個順序、刻意不做什麼、每次審查找到什麼 |
| [ENGINEERING_RULES.md](ENGINEERING_RULES.md) | 所有階段都要遵守的工程規則，以及隨階段改變的那幾條 |
| [API_CONTRACT.md](API_CONTRACT.md) | core 與 executor（也就是架構說明裡的 actor）之間的契約 |
| [CLIENT_CONTRACT.md](CLIENT_CONTRACT.md) | client 的 ID、sequence、重試與去重 |
| [COMMIT_RULE_REVIEW.md](COMMIT_RULE_REVIEW.md) | commit 規則與 Figure 8 的推導（V1 時期撰寫，開頭註明哪裡改了） |
| [FAULT_SCHEDULE_FORMAT.md](FAULT_SCHEDULE_FORMAT.md) | 故障排程的格式與重現（V1 時期撰寫，仍適用） |
| [ACCEPTANCE_FINAL.md](ACCEPTANCE_FINAL.md) | 目前有效的驗收契約怎麼讀、怎麼跑 |
| [reference/raft-notes.md](reference/raft-notes.md) | Raft 論文與博士論文的章節筆記與出處 |

## 規格與補記

每個階段先寫規格、取得同意再實作；實作後的結果寫在同一份的補記。

| 文件 | 階段 |
|---|---|
| [STORAGE_FORMAT_V2.md](STORAGE_FORMAT_V2.md) | S1 儲存格式；也是目前的 WAL 格式說明 |
| [SNAPSHOT_COMPACTION.md](SNAPSHOT_COMPACTION.md) | S2 快照與 log 壓縮 |
| [READ_INDEX.md](READ_INDEX.md) | S3 ReadIndex 讀取 |
| [MEMBERSHIP.md](MEMBERSHIP.md) | S4 成員變更 |
| [PERFORMANCE.md](PERFORMANCE.md) | S5 效能；S6 收尾在 §15 |

S10（最後一次複查）沒有新功能，找到與修正的事寫在 [開發歷程](DEVELOPMENT.md) 與教學 20、21。

## 紀錄

| 文件 | 內容 |
|---|---|
| [INDEPENDENT_REVIEW_S1_S4.md](INDEPENDENT_REVIEW_S1_S4.md) | S1～S4 的獨立審查與處理結果 |
| [INDEPENDENT_REVIEW_S5.md](INDEPENDENT_REVIEW_S5.md) | S5 的獨立審查 |
| [TEST_RECORD_20261114.md](TEST_RECORD_20261114.md) | 2026-11-14 實際跑過的測試與沒做的事 |
| [ENGINEERING_HARDENING.md](ENGINEERING_HARDENING.md) | V1 時期的工程強化 |

## 歷史

| 文件 | 被哪份取代 |
|---|---|
| [IMPLEMENTATION_REPORT.md](IMPLEMENTATION_REPORT.md) | V1 的實作報告 → [ARCHITECTURE.md](ARCHITECTURE.md) 與各階段規格 |
| [VERIFICATION_FINDINGS.md](VERIFICATION_FINDINGS.md) | V1 的驗證紀錄 → [ACCEPTANCE_FINAL.md](ACCEPTANCE_FINAL.md) 與審查報告 |
| [ACCEPTANCE_V1.md](ACCEPTANCE_V1.md) | 替代驗收 v1 → [ACCEPTANCE_FINAL.md](ACCEPTANCE_FINAL.md) |
| [STORAGE_FORMAT_V1.md](STORAGE_FORMAT_V1.md) | WAL 格式 1 → [STORAGE_FORMAT_V2.md](STORAGE_FORMAT_V2.md) |
| `../diagrams/` | V1 時期的兩張圖 → [ARCHITECTURE.md](ARCHITECTURE.md) 的四張圖 |

## 原始材料

[reference/](reference/README.md) 除了 `raft-notes.md` 之外，是專案開始時拿到的規格套件（2026-11-10），保留原樣。根目錄的 `START_HERE_zh-TW.md`、`ALL_IN_ONE_SPEC.md`、`01_FEASIBILITY_REVIEW.md` 是三份原始規格，同樣不修改。
這份套件原本是設計給程式生成工具逐階段執行的，所以裡面提到的執行器與 `scripts/run_phase.sh` 的用法，和這個儲存庫實際的驗收方式不同（見 [ACCEPTANCE_V1.md](ACCEPTANCE_V1.md)）。
