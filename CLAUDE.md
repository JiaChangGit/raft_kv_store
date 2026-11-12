# Claude Code 專案入口

先閱讀 [接手與未來計畫](docs/HANDOFF_CLAUDE.md)，再依任務按需閱讀
[AGENTS.md](AGENTS.md)、[README.md](README.md) 和相關契約。使用者最新指示與宿主政策優先。

- 以自然、清楚的台灣繁體中文溝通；進行較長工作時定期回報進度，說明實際證據與限制。
- 本專案位於 WSL `/home/user/src/Distributed_Key-Value_Store_with_Raft`；相鄰
  `ai-general-framework` 保持唯讀。不可覆寫三份原始規格，亦不可為了通過 gate 降低要求。
- P00～P08 與工程強化已有歷史通過紀錄；新增本交接文件使來源指紋改變。執行
  `.venv/bin/python scripts/status.py` 查看當前狀態，勿將舊 receipt 當成目前來源的通過證據。
- 專案目的是**學習與面試展示**。整體計畫、已確認的決定、階段順序與刪減方案見
  [最終版路線圖](docs/ROADMAP.md)：只做一個最後版本（不做新舊格式並存與遷移），範圍含 WAL header
  integrity、snapshot 與壓縮、ReadIndex、membership change、效能優化；開發中遇到的 bug 寫成
  新手導向教學。路線圖已於 2026-11-12 獲使用者確認；進度以 Git tag 與 `scripts/status.py` 為準。
- 有效的驗收契約是 `acceptance/final.json`（說明見 `docs/ACCEPTANCE_FINAL.md`），receipt 在
  `artifacts/acceptance-final/`；`acceptance/v1.json` 與 `artifacts/acceptance-v1/` 是歷史紀錄。
  下方較早的條目若提到「P00～P08 的舊 receipt」，指的是 v1 時期。
- 寫 bug 教學用專案自有的 `write-bug-lesson` skill（`.claude/skills/`），樣板與規則在 `docs/lessons/`；
  Raft 論文出處查 `docs/reference/raft-notes.md`，引用前回原文核對。
- S0（tag `s0-foundation-verified`）與 S1（tag `s1-storage-format-verified`）已完成。儲存層只有格式 2
  （`docs/STORAGE_FORMAT_V2.md`）；格式 1 的程式只存在於 tag `v1-baseline-verified-20261112`，`STORAGE_FORMAT_V1.md` 是歷史文件。
  S2（tag `s2-snapshot-compaction-verified`）已完成，規格與實作補記在 `docs/SNAPSHOT_COMPACTION.md`。
  下一個階段是 S3（ReadIndex），開始前先寫規格並取得使用者同意。
- 下方 `AGENTS.md` 仍記載 V1 的禁止事項（snapshot、membership change、ReadIndex、固定三節點）；
  依路線圖，各階段開始時才改寫對應條目，未到的階段仍須遵守。
- 框架的增刪修候選與驗證方式另見 [框架改進建議](docs/FRAMEWORK_IMPROVEMENT_PROPOSALS.md)；
  那是供討論的建議，不代表已授權修改相鄰框架。
- 框架資源（2026-11-12 以 `agf project update --host claude` 配置）：skills 在 `.claude/skills/`
  （debug-root-cause、guide-learning、read-research、review-change、trace-code），與
  `.agents/skills/` 內容相同，皆由框架管理，不要直接修改副本。協作約定在
  `.agf/managed/project-rules.md`，按需指引在 `.agf/managed/guides/`；CLI 為
  `python3 -B /home/user/src/ai-general-framework/scripts/agf.py`（加 `-B` 避免在唯讀框架寫入快取）。
- 使用者想藉本專案的開發過程改進框架：凡是與框架、skill、宿主載入、交接或驗收工具有關的
  實際經驗（含順利與卡住的），當下追記到 [框架經驗紀錄](docs/FRAMEWORK_EXPERIENCE_LOG.md)，
  依該檔的欄位寫明證據與可歸因範圍；不要事後憑印象補寫，也不要因此修改框架。
- `docs/`、`scripts/`、`acceptance/` 與程式都計入來源指紋，改動後既有 receipt 即不適用於目前來源；
  `.claude/` 與 `.agents/` 自 2026-11-12 起排除在指紋外。
  `.claude/settings.json` 以 deny 規則擋下框架目錄的寫入與 push／建立 PR。
- 保留使用者資料、現有未追蹤檔案及本機證據；不自行 push、建立 PR 或部署。
  測試、工具、框架配置的實際狀態與適用權限請依交接文件和當前環境確認。
