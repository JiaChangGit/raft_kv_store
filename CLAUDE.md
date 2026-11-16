# Claude Code 專案入口

先閱讀 [接手與未來計畫](docs/HANDOFF_CLAUDE.md)，再依任務按需閱讀
[AGENTS.md](AGENTS.md)、[README.md](README.md) 和相關契約。使用者最新指示與宿主政策優先。

框架的協作約定每個 session 都載入（2026-11-16 起；S8 發現少了這一行時，規則從沒被讀到）：

@.agf/managed/project-rules.md

- 以自然、清楚的台灣繁體中文溝通；進行較長工作時定期回報進度，說明實際證據與限制。
- 本專案在 WSL 上。相鄰目錄的 `ai-general-framework`（`../ai-general-framework`）保持唯讀，除非使用者在當次對話點名；
  2026-11-16 點名的框架修改做在分支 `s8-follow-up`（worktree `../agf-worktrees/s8-follow-up`），框架本身的工作目錄不動。
  不可覆寫三份原始規格，亦不可為了通過 gate 降低要求。
- 執行 `.venv/bin/python scripts/status.py` 查看每個階段最近一次驗收、是否對應目前的來源；
  勿將舊 receipt 當成目前來源的通過證據。
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
  S3（tag `s3-read-index-verified`）已完成，規格與補記在 `docs/READ_INDEX.md`；新的 GET 走 ReadIndex、不寫 log、不去重。
  S4（tag `s4-membership-verified`）已完成，規格與補記在 `docs/MEMBERSHIP.md`：節點編號 1～7、設定在 log 中、一次一台；
  叢集由一台 `--bootstrap`、其他以 `tools/kv_admin.py` 逐一加入（`tools/cluster.py` 自動這樣做）。論文第 4 章與 2015 年郵件的摘錄在 `docs/reference/raft-notes.md`。
  S5（tag `s5-performance-verified`）已完成，規格與補記在 `docs/PERFORMANCE.md`：group commit、批次複製、落盤只帶變動（`Delta`）；
  效能量測用 `scripts/write_benchmark.py`，不要經 `Cluster.invoke` 量時間（教學 14）。2026-11-15 使用者縮減了後續計畫：S6 只補 S5 審查的 F2～F5、驗收一次；之後是 S7（中文 AI 味檢查）、
  S8（框架分析與對照實驗）、S9（公開整理）。細節見路線圖「2026-11-15 的計畫修改」。
  S6（tag `s6-review-fixes-verified`）的規格與補記在 `docs/PERFORMANCE.md` §15，驗收契約是新的階段 15，教學 16～19。
  S7（tag `s7-zh-style-prototype`）已完成，成果在 `framework-study/zh-style/`（先讀 `report.md`）；不屬於 Raft，不在驗收契約內。
  S8（tag `s8-framework-analysis`）已完成：元件盤點、三個對照實驗與分析報告在 `framework-study/analysis/`（先讀 `report.md`）。
  S8 後續（tag `s8-framework-changes`）已完成：框架的修改在分支 `s8-follow-up`，規格、探測與結果在 `framework-study/changes/`（先讀 `RESULTS.md`）；
  其中三件事等使用者決定（該檔最後一節）。下一個是 S9（公開整理），開始前先寫規格並取得使用者同意。
- S1～S4 的獨立審查已於 2026-11-14 補做並修正（紀錄 `docs/INDEPENDENT_REVIEW_S1_S4.md`，教學 11～13）；S1 的審查只做到不同 session。
  改模擬器的工作負載或設定前，先讀教學 11、12：固定的參數與「挑最合理的對象」都曾讓整條路徑測不到。
  寫任何「逐一植入錯誤再建置」的腳本前，先讀教學 06（共用建置目錄會互相污染）。
- `AGENTS.md` 的 V1 禁止事項依路線圖在各階段開始時改寫（S2 snapshot、S3 ReadIndex、S4 成員變更已改）；
  仍有效的是不使用既有 Raft 函式庫、不用 unsafe、不做 lease。
- 框架的分析與建議見 [S8 報告](framework-study/analysis/report.md)，依報告做的修改見 [S8 後續的規格](framework-study/changes/SPEC.md)；
  舊的 `docs/FRAMEWORK_IMPROVEMENT_PROPOSALS.md` 已被 S8 報告取代。
- 框架資源（2026-11-12 以 `agf project update --host claude` 配置）：skills 在 `.claude/skills/`
  （debug-root-cause、guide-learning、read-research、review-change、trace-code），與
  `.agents/skills/` 內容相同，皆由框架管理，不要直接修改副本。**用 Skill 工具叫用這些 skill**，不要直接讀
  `.agents/skills/` 的副本（S8 實驗 A：照舊規則自己讀檔，Skill 工具一次都沒被叫用）。協作約定由上方的 import 載入，
  按需指引在 `.agf/managed/guides/`；CLI 為 `python3 -B ../ai-general-framework/scripts/agf.py`（加 `-B` 避免在唯讀框架寫入快取）。
- 使用者想藉本專案的開發過程改進框架：凡是與框架、skill、宿主載入、交接或驗收工具有關的
  實際經驗（含順利與卡住的），當下追記到 [框架經驗紀錄](docs/FRAMEWORK_EXPERIENCE_LOG.md)，
  依該檔的欄位寫明證據與可歸因範圍；不要事後憑印象補寫，也不要因此修改框架。
- `docs/`、`scripts/`、`acceptance/` 與程式都計入來源指紋，改動後既有 receipt 即不適用於目前來源；
  `.claude/` 與 `.agents/` 自 2026-11-12 起排除在指紋外。
  `.claude/settings.json` 以 deny 規則擋下框架目錄的寫入與 push／建立 PR。**這些規則只在從專案目錄啟動 Claude Code 時生效**：
  2026-11-16 實測，從上層目錄啟動的 session 不會載入它們（`framework-study/changes/RESULTS.md`）。
- 保留使用者資料、現有未追蹤檔案及本機證據；不自行 push、建立 PR 或部署。
  測試、工具、框架配置的實際狀態與適用權限請依交接文件和當前環境確認。
