# Claude Code 專案入口

先閱讀 [接手與未來計畫](docs/HANDOFF_CLAUDE.md)，再依任務按需閱讀
[AGENTS.md](AGENTS.md)、[README.md](README.md) 和相關契約。使用者最新指示與宿主政策優先。

- 以自然、清楚的台灣繁體中文溝通；進行較長工作時定期回報進度，說明實際證據與限制。
- 本專案位於 WSL `/home/user/src/Distributed_Key-Value_Store_with_Raft`；相鄰
  `ai-general-framework` 保持唯讀。不可覆寫三份原始規格，亦不可為了通過 gate 降低要求。
- P00～P08 與工程強化已有歷史通過紀錄；新增本交接文件使來源指紋改變。執行
  `.venv/bin/python scripts/status.py` 查看當前狀態，勿將舊 receipt 當成目前來源的通過證據。
- 下一個已約定的方向是**先提出 V2 範圍與驗收設計，和使用者討論**；不要直接實作
  V2 或假設 snapshot、compaction 已獲採納。WAL header integrity 是有實際反例的優先候選。
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
- `.claude/`、`.agents/` 與 `docs/` 都計入來源指紋；改動後舊 receipt 即不適用於目前來源。
  `.claude/settings.json` 以 deny 規則擋下框架目錄的寫入與 push／建立 PR。
- 保留使用者資料、現有未追蹤檔案及本機證據；不自行 push、建立 PR 或部署。
  測試、工具、框架配置的實際狀態與適用權限請依交接文件和當前環境確認。
