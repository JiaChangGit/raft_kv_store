# 文件地圖

每份文件屬於下面四類之一。「紀錄」寫的是當時發生的事，保留原文，不隨程式更新；裡面的路徑是作者機器上的路徑。「歷史」是已被取代的版本，開頭都有標示，指向現在以哪份為準。

## 現行說明

| 文件 | 內容 |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | 元件、一次寫入與一次讀取的流程、成員變更；附 Mermaid 圖與對照的程式位置 |
| [LEARNING_GUIDE.md](LEARNING_GUIDE.md) | 從一筆 PUT 看懂整個系統，附練習 |
| [HANDS_ON.md](HANDS_ON.md) | 實際操作：三個 process 選出 leader、寫入、終止 leader、恢復 |
| [lessons/](lessons/README.md) | 19 篇 bug 教學（草稿），每篇都有重現步驟與當時的輸出 |
| [API_CONTRACT.md](API_CONTRACT.md) | core 與 executor 之間的契約 |
| [CLIENT_CONTRACT.md](CLIENT_CONTRACT.md) | client 的 ID、sequence、重試與去重 |
| [STORAGE_FORMAT_V2.md](STORAGE_FORMAT_V2.md) | 目前的 WAL 格式與持久化契約（也是 S1 的規格） |
| [COMMIT_RULE_REVIEW.md](COMMIT_RULE_REVIEW.md) | commit 規則與 Figure 8 的推導（V1 時期撰寫，開頭註明哪裡改了） |
| [FAULT_SCHEDULE_FORMAT.md](FAULT_SCHEDULE_FORMAT.md) | 故障排程的格式與重現（V1 時期撰寫，仍適用） |
| [ACCEPTANCE_FINAL.md](ACCEPTANCE_FINAL.md) | 目前有效的驗收契約怎麼讀、怎麼跑 |
| [AI_COLLABORATION.md](AI_COLLABORATION.md) | 這個專案怎麼和 AI 一起做：分工、流程、審查、限制 |
| [reference/raft-notes.md](reference/raft-notes.md) | Raft 論文與博士論文的章節筆記與出處 |

## 規格與補記

每個階段先寫規格、取得同意再實作；實作後的結果寫在同一份的補記。

| 文件 | 階段 |
|---|---|
| [ROADMAP.md](ROADMAP.md) | 整體計畫、每階段的固定流程、計畫的修改 |
| [STORAGE_FORMAT_V2.md](STORAGE_FORMAT_V2.md) | S1 儲存格式 |
| [SNAPSHOT_COMPACTION.md](SNAPSHOT_COMPACTION.md) | S2 快照與 log 壓縮 |
| [READ_INDEX.md](READ_INDEX.md) | S3 ReadIndex 讀取 |
| [MEMBERSHIP.md](MEMBERSHIP.md) | S4 成員變更 |
| [PERFORMANCE.md](PERFORMANCE.md) | S5 效能；S6 收尾在 §15 |
| [PUBLIC_RELEASE.md](PUBLIC_RELEASE.md) | S9 公開整理 |
| `../framework-study/*/SPEC.md` | S7、S8 與 S8 後續（框架研究，見 [framework-study/](../framework-study/README.md)） |

## 紀錄

| 文件 | 內容 |
|---|---|
| [INDEPENDENT_REVIEW_S1_S4.md](INDEPENDENT_REVIEW_S1_S4.md) | S1～S4 的獨立審查與處理結果 |
| [INDEPENDENT_REVIEW_S5.md](INDEPENDENT_REVIEW_S5.md) | S5 的獨立審查 |
| [TEST_RECORD_20261114.md](TEST_RECORD_20261114.md) | 2026-11-14 實際跑過的測試與沒做的事 |
| [FRAMEWORK_EXPERIENCE_LOG.md](FRAMEWORK_EXPERIENCE_LOG.md) | 使用 ai-general-framework 的當下紀錄（只往後追加） |
| [HANDOFF_CLAUDE.md](HANDOFF_CLAUDE.md) | 給下一個 AI session 的接手文件 |
| [ENGINEERING_HARDENING.md](ENGINEERING_HARDENING.md) | V1 時期的工程強化 |
| [WORKLOG.md](WORKLOG.md) | V1 時期的工作紀錄 |

## 歷史

| 文件 | 被哪份取代 |
|---|---|
| [IMPLEMENTATION_REPORT.md](IMPLEMENTATION_REPORT.md) | V1 的實作報告 → [ARCHITECTURE.md](ARCHITECTURE.md) 與各階段規格 |
| [VERIFICATION_FINDINGS.md](VERIFICATION_FINDINGS.md) | V1 的驗證紀錄 → [ACCEPTANCE_FINAL.md](ACCEPTANCE_FINAL.md) 與審查報告 |
| [ACCEPTANCE_V1.md](ACCEPTANCE_V1.md) | 替代驗收 v1 → [ACCEPTANCE_FINAL.md](ACCEPTANCE_FINAL.md) |
| [STORAGE_FORMAT_V1.md](STORAGE_FORMAT_V1.md) | WAL 格式 1 → [STORAGE_FORMAT_V2.md](STORAGE_FORMAT_V2.md) |
| [FRAMEWORK_FEEDBACK.md](FRAMEWORK_FEEDBACK.md)、[FRAMEWORK_IMPROVEMENT_PROPOSALS.md](FRAMEWORK_IMPROVEMENT_PROPOSALS.md) | 早期的框架回饋 → [S8 的分析報告](../framework-study/analysis/report.md) |

## 原始材料

[reference/](reference/README.md) 除了 `raft-notes.md` 之外，是專案開始時拿到的規格套件（2026-11-10），保留原樣。根目錄的 `START_HERE_zh-TW.md`、`ALL_IN_ONE_SPEC.md`、`01_FEASIBILITY_REVIEW.md` 是三份原始規格，同樣不修改。
