# 最終版路線圖

更新日期：2026-11-12。狀態：使用者已確認；S0～S3 已完成。本文件是之後每個 session 的共同依據；
實際進度以 Git tag、驗收 receipt 與 `scripts/status.py` 為準，不以本文件的文字為準。

## 專案目的與由此而來的取捨

目的是**學習**與**面試時展示能力**，並藉開發過程改進使用者自己的 AI 框架。因此：

- 只做一個「最後一版」：不維護新舊格式並存，不做遷移工具與 rollback。舊版以 Git tag 保存，
  需要重現舊 bug 時切回 tag。
- 程式由 AI 完成；每個階段附新手導向的教學與練習，讓使用者事後能跟著做一遍並講給別人聽。
- 開發中遇到的每個 bug 都寫成教學文件（見「Bug 教學」）。
- 功能之間若有衝突或無法達到驗收標準，AI 可自行修改設計或刪減，但必須寫明刪了什麼、為什麼。
- 最後整理成可公開的 GitHub 儲存庫，README 與架構總覽中英並列，教學維持繁體中文。push 由使用者執行。

不變的底線：三份原始規格保留原文；不削弱測試來換取通過；分清楚「已執行通過」「推論」「未驗證」；
不宣稱 production-ready 或形式化證明；相鄰的 ai-general-framework 唯讀。

## 已確認的決定

| 項目 | 決定 |
|---|---|
| 儲存格式 | 只保留新格式；v1 讀寫程式移除，舊版在 tag `v1-baseline-verified-20261112` |
| 驗收 | 整併成一份最終契約；`acceptance/v1.json` 與既有 receipt 留作歷史紀錄，不再是門檻。v1 契約裡每一項性質都要在新契約中有對應且不較弱的測試，對照表寫進契約 |
| 功能 | WAL header integrity、Snapshot 與 log compaction、ReadIndex、Membership change、效能優化 |
| 學習方式 | AI 全寫＋教學與練習 |
| 展示 | 公開 GitHub，中英並列 |

仍待確認（附建議）：

- **來源指紋是否排除 `.claude/` 與 `.agents/`**：建議排除。它們是框架管理的副本，另有
  `agf project status` 檢查是否被改動；排除後更新框架不會讓驗收紀錄失效。新契約本來就要改 checker，一併處理。
- **`AGENTS.md` 的規則更新**：目前明文禁止 snapshot、membership change、ReadIndex，並規定固定三節點與
  「所有 GET 都寫進 log」。每個階段開始時改寫對應的那幾條，其餘不動。

## 階段

順序由相依關係決定：壓縮需要可信的儲存格式；新節點加入需要 snapshot 才追得上；
ReadIndex 與 membership 都依賴「目前的成員是誰、多數是幾台」；效能最後做，才有穩定的基準可比。

| 階段 | 內容 | 主要風險與和其他功能的交互作用 |
|---|---|---|
| S0 基礎 | 最終驗收契約與 checker、`AGENTS.md` 規則更新流程、Bug 教學樣板與專案 skill、參考資料筆記 | checker 改動後須從頭重驗 |
| S1 儲存格式（**已完成**，tag `s1-storage-format-verified`） | header CRC、檔頭、殘尾保留、可接續的初始化。檔頭已預留 snapshot 基準（base index／term、起點的 term 與投票、起始 sequence）；log entry 的種類在 JSON 內擴充，不需改外框。規格見 `STORAGE_FORMAT_V2.md`，教學見 `lessons/01-wal-length-corruption/` | 「每次 `save` 都複製並比對整份 log」原訂在此處理，**改到 S2**：讀程式後發現 core 每個事件也複製並比對整份狀態，只改儲存層無效，應與 S2 的 log 結構重構一起做 |
| S2 Snapshot 與壓縮（**已完成**，tag `s2-snapshot-compaction-verified`） | 狀態機快照（KV＋每個 client 最新一筆的 session）、WAL 從快照之後重寫、分塊的 InstallSnapshot、落後節點追趕、安裝中斷的往前完成。規格 `SNAPSHOT_COMPACTION.md`，教學 02、03。「每個事件複製整份狀態」依決定 D4 延到 S5 | 目前到處假設「log 的位置＝index」，要全面改成帶起始位移；這是影響面最大的一次重構 |
| S3 ReadIndex（**已完成**，tag `s3-read-index-verified`） | GET 不寫入 log：leader 先 commit 本任期 entry，記下 readIndex，以讀取到達後送出的一輪 AppendEntries 取得多數確認，套用到 readIndex 後回覆；GET 不再去重。規格 `READ_INDEX.md`。量測：GET 不再產生 WAL record | 多數的計算集中在一處，S4 改為依成員設定計算。**S4 待辦**：follower 數少於多數所需時，ReadIndex 必須不確認任何回合（S3 已改預設值，但固定三台時無法測試） |
| S4 Membership change | 一次增減一台（單步變更）；設定以 log entry 表示，寫入即生效；新節點靠 snapshot 追上；leader 被移除後退位 | 固定 ID 1/2/3 與 quorum 2 的假設散布在 core、transport、設定檔與測試；單步變更有一個已知的安全性陷阱（leader 必須先在本任期 commit 過 entry） |
| S5 效能 | group commit（多筆交易共用一次 fsync）、批次複製、前後 benchmark 對照 | 不得改變「先持久化、後回覆」的次序 |
| S6 整體強化 | 所有功能同時開啟的模擬（多種子、長時間）、故障注入下的線性一致性、mutation、由不同模型做獨立審查、乾淨副本重建 | 功能兩兩交互作用的 bug 最可能在這裡出現 |
| S7 公開整理 | 中英 README、架構圖、教學索引、移除本機絕對路徑、AI 協作說明 | 不改程式行為 |

每個階段的固定流程：規格（含不保證事項）→ 會失敗的測試 → 實作 → 模擬與故障測試 → 獨立審查 →
教學文件 → 驗收 receipt → 本機 commit 與 tag → **停下來讓使用者提問**。

### 預先聲明的刪減方案

「沒有 bug」無法保證；能做到的是：每項功能都通過模擬器的不變量檢查、故障注入下的線性一致性檢查、
mutation 測試與獨立審查，並把沒驗到的部分寫清楚。達不到這個標準的功能不會以「可用」的名義留在最終版：

- Membership change 若無法通過 S6：保留設計文件與教學，功能從最終版移除，叢集維持固定三節點。
- 不做：joint consensus（一次改多台）、lease read、leader transfer、learner（不投票的追趕節點）、
  PreVote。其中若有哪一項被證明是其他功能正確性所必需，會在該階段提出並說明。
- 效能優化若與安全性檢查衝突，以安全性為準，保留 benchmark 數字與原因。

## Bug 教學

位置 `docs/lessons/NN-簡短名稱/`，每個 bug 一個資料夾。讀者設定為**沒寫過分散式系統的新手**。固定章節：

1. 一句話說明這個 bug 會造成什麼後果。
2. 需要的背景知識（用白話解釋每個術語，第一次出現時附原文）。
3. 親手重現：可直接執行的命令、預期看到的輸出、每一步在做什麼。
4. 為什麼會這樣：沿著程式一步步追，附流程圖或時序圖。
5. 為什麼這很嚴重：對應到 Raft 的哪一條安全性質。
6. 怎麼修、考慮過哪些做法、為什麼選這個。
7. 怎麼證明修好了：哪些測試、各自擋住什麼。
8. 還有什麼沒解決。
9. 自我檢查題與一個動手練習（附解答位置）。
10. 面試時怎麼講：兩分鐘版本的重點。

第一篇是已重現的 WAL 長度欄位毀損。工程強化階段找到的九個問題（見 `ENGINEERING_HARDENING.md`）
在 S0 評估哪些有足夠的保存證據可以補寫。教學中的每個命令都要實際跑過，輸出取自實際結果。

## 會用到的工具

| 類別 | 項目 | 用在哪裡 |
|---|---|---|
| 框架 skill | `read-research` | 每階段開始前讀 Raft 論文與博士論文的對應章節 |
| | `trace-code` | S2、S4 重構前，找出所有依賴舊假設的程式位置 |
| | `debug-root-cause` | 每個 bug 的根因與證據，直接作為教學素材 |
| | `review-change` | 每階段結束的獨立審查 |
| | `guide-learning` | 教學與練習的設計 |
| 專案 skill（新增） | `write-bug-lesson` | 上述十個章節的樣板與檢查表。若好用，列為回饋給框架的候選 |
| Scripts（新增） | `scripts/check_lessons.py` | 檢查每篇教學章節齊全、重現命令存在且可執行 |
| | 各教學資料夾內的重現腳本 | 讓讀者一行命令看到 bug |
| Scripts（修改） | `verify_gate.py`、`status.py`、`revalidate_all.py`、`mutation_check.py` | 支援最終契約 |
| 圖解 | Mermaid 圖源，寫在 Markdown 的 `mermaid` 區塊（GitHub 直接顯示） | 架構圖、時序圖。本機的 `mmdc` 設定指向 Windows 的瀏覽器，在 WSL 內無法渲染；需要圖檔時要另外處理 |
| Reference（新增） | `docs/reference/raft-notes.md` | 論文章節對照筆記與出處連結：Raft 擴充版論文、Ongaro 博士論文（成員變更、壓縮、唯讀請求各章）、單步成員變更的已知問題說明。只放筆記與連結，不放論文全文；內容以實際讀取的原文為準 |
| MCP | 不新增 | 純本機開發；讀論文用宿主內建的網頁讀取即可。之後若出現具體缺口再評估並記錄 |

Skills 目前已配置但尚未確認宿主會載入；下一個 session 請從專案根目錄啟動並先確認。

## 建議的模型分工

- 規格與設計（各階段開頭，尤其 S2、S4）：推理最強的模型。
- 依規格實作與測試：中階模型即可。
- 獨立審查：與實作不同的模型，另開 session，不提供預期答案。
- 教學文件：中階模型，寫完後由使用者以新手角度實際照做一遍，卡住的地方就是要改的地方。

## 規模

這是多個 session 的工作量。S2 與 S4 各自的規模都不小於整個 V1 的儲存與複製部分。
每個階段獨立成立：做到哪一階段結束，專案都處於「已驗收、有教學、可展示」的狀態。
