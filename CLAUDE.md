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
- 保留使用者資料、現有未追蹤檔案及本機證據；不自行 push、建立 PR 或部署。
  測試、工具、框架配置的實際狀態與適用權限請依交接文件和當前環境確認。
