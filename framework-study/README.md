# 框架研究（不屬於 Raft 系統）

這個資料夾放的是分析使用者的 AI 框架（`ai-general-framework`，本專案不修改它）所做的實驗與報告，和 Raft KV store 的程式、驗收契約無關。
計畫與依據見 [路線圖](../docs/ROADMAP.md)「2026-11-15 的計畫修改」。

| 資料夾 | 階段 | 內容 |
|---|---|---|
| [zh-style/](zh-style/) | S7 | 中文「AI 味」：實例分析、對照表、規則式提示腳本與它的試跑報告。先讀 [report.md](zh-style/report.md) |
| [analysis/](analysis/) | S8 | 框架的元件盤點、三個對照實驗與分析報告。先讀 [report.md](analysis/report.md) |

這裡的檔案計入 Raft 驗收的來源指紋；加入之後 `scripts/status.py` 會顯示 `source changed`，S9 收尾時從 P00 重跑一次。
