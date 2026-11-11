# Claude Code 接手與未來計畫

更新日期：2026-11-10。此文件交接 Rust Raft KV Store 的已知狀況、專案約定與後續
討論方向。使用者希望從 Codex 轉交 Claude Code；請先核對現況，不要把此文件當成
永久不變的狀態。**下一步是與使用者討論 V2 設計，並非直接開始 V2 實作。**

## 一開始先做什麼

在 WSL/Linux 執行，路徑若因環境變動應先查明：

```bash
cd /home/user/src/Distributed_Key-Value_Store_with_Raft
git status --short
.venv/bin/python scripts/status.py
cat AGENTS.md
```

接著閱讀 `docs/ENGINEERING_HARDENING.md`、`docs/STORAGE_FORMAT_V1.md`，按需看
`docs/WORKLOG.md`、`docs/API_CONTRACT.md`、`docs/CLIENT_CONTRACT.md` 及實作。若
`.venv`、`.agf` 或 `artifacts` 不存在，先如實標示；它們被 `.gitignore` 排除，不會
因複製 Git 來源自動帶上。不要因歷史文件說曾安裝過就假設新環境已可用。

目前 Git 儲存庫尚無提交，`git status` 顯示整個專案來源為未追蹤檔案。**這不代表
這些檔案可以刪除或覆蓋**，也無法直接以一般的 `git diff` 比較已提交基準。已保存的
`artifacts/reports/engineering-stage2-source.zip` 是工程強化結束時的來源快照；它不
包含本交接文件，也不包含忽略的工作資料或測試證據。資料、證據與其他來源檔應分別
保護；是否建立本機 Git commit 依使用者當下授權與需求決定。

## 使用者意圖與協作方式

- 建立並理解一個 Rust Raft KV Store；用自然、通順的台灣繁體中文解釋，說明程式
  與測試的因果，避免空泛文字。適合時沿一筆請求追蹤狀態、對照規格與論文，並搭配
  流程圖、時序圖、反例或練習。現成教學見 `docs/HANDS_ON.md` 與 `docs/LEARNING_GUIDE.md`。
- 已按順序完成 V1、實際操作與教學驗收、工程強化。下一階段應先設計並討論 V2 的
  範圍與驗收；使用者原先要求分階段進行、適合在階段結束時提問。
- 使用者已允許目標專案所需的本機實作、測試與必要環境準備；不要重問已明確授權的
  普通操作。push、建立 PR、部署及新的外部連線仍須符合當次明確指示和宿主政策。
  不要在交接檔、命令紀錄或測試資料中記錄密碼或其他憑證。
- 修改限於本目標專案；相鄰 `/home/user/src/ai-general-framework` 保持唯讀。
  不假設框架是模型協調器、有永久記憶或會自動學習；不預設新增 MCP 或搜尋索引。
- 保留 `START_HERE_zh-TW.md`、`01_FEASIBILITY_REVIEW.md`、`ALL_IN_ONE_SPEC.md`
  原文。原始 ZIP、原始 `gates.json` 與腳本未取得；`docs/reference/` 是從現有材料
  抽出的方便閱讀副本，`acceptance/v1.json` 是本專案訂出的**替代驗收**，不是原 gate。

## V1 的結構與不變條件

固定三節點 ID 1/2/3、quorum 2、單一 Raft group。`raft-core` 為純事件到有序效果
的核心，模擬器與 runtime 共用它；`raft-node` 的單一 actor 擁有共識與狀態機狀態。
模組定位見 `README.md`。修改前先讀受影響程式、呼叫者、對應契約和測試。

目前最重要的工程條件見 `AGENTS.md`：依賴 term/vote/log 的效果前必須持久化，
storage 失敗停止服務；leader 直接推進 commit 需要多數 durable match 與 current-term
entry。回復不能將所有 WAL 視為 committed。新 GET 走 replicated log；回覆成功須在
commit 和本地 apply 後。重試依 replicated client ID、sequence、canonical command
與原結果去重。故障測試的節點間流量及 reconnect 經完整 frame proxy。

V1 不使用既有 Raft library、unsafe Rust、snapshot、動態 membership、lease 或
ReadIndex。若未來決議改變這些範圍，先明確更新設計、專案規則、相容性及驗收，而非
把舊 V1 規則當成 V2 已完成或自行繞過。WAL 與 session 現在持續成長；永久失去多數
不保證服務可用。請勿聲稱此教學專案已達 production-ready 或協定已被形式化證明。

## 實作與文字風格

- 沿用既有 Rust workspace、Rust 2024 edition、`rustfmt` 和 clippy；先選擇可讀的最小
  變更。保持純 core 與 OS I/O 邊界、actor 所有權、明確錯誤及有界資源；不要用測試
  專屬捷徑改變正式資料路徑，也不要為假想需求引入新框架或抽象層。
- 測試要檢查外部可觀察行為與安全條件，含可重現的負向案例；避免只重述實作或只
  檢查退出碼。保留首次失敗、命令、環境、結果、失敗原因與修正後證據。
- 文件與回報分清程式中存在、實際執行且通過、尚未驗證及合理推論。保留原術語，
  首次出現時用白話解釋；教學圖源與渲染方式遵循 `.agf/managed/project-rules.md`
  及按需指引，圖解內容以實際來源為依據。
- 禁止為了通過 gate 削弱測試、忽略失敗或悄悄變更驗收條件。修改安全性條件、
  協定或資料格式前，先整理相容性與失敗路徑，留下可由他人檢查的決策。

## 框架在 Claude Code 中的使用

既有框架入口為 `/home/user/src/ai-general-framework/scripts/agf.py`，目標專案
目前有 `.agf/managed/project-rules.md` 與 `.agents/skills/` 中的 read-research、
trace-code、review-change、debug-root-cause、guide-learning。它們被記錄為曾經
實際讀用的資源；**不能因此宣稱 Claude Code 已自動發現或載入這些 skills**。
若檔案仍存在，可按任務直接讀取相應 `SKILL.md`，並依框架規則讀取 profile：

```bash
python3 -B /home/user/src/ai-general-framework/scripts/agf.py profile show --root /home/user/src/Distributed_Key-Value_Store_with_Raft
```

先檢查目前 Claude 宿主的可用功能、專案既有規則和框架 CLI 說明；不要套用未驗證
的宿主設定，不要覆寫此 `CLAUDE.md` 或 `AGENTS.md`。若需要實際配置，先預覽和檢查
受影響檔案，再依當前授權操作；配置成功、宿主發現 skill、真實任務有效是三件事。
框架觀察與可驗假說見 `docs/FRAMEWORK_FEEDBACK.md`；沒有對照資料就不宣稱框架造成
成效，也不要把 Raft 專屬規則硬塞進通用框架。
完整增刪修候選、觀察分層及驗證矩陣另見 `docs/FRAMEWORK_IMPROVEMENT_PROPOSALS.md`。

## 驗收與證據的目前狀態

2026-11-10 工程強化完成時，WSL 執行的 Rust 34 項與 Python 45 項通過；替代驗收
P00～P08 均對應當時來源指紋
`5a61deeecd87efc373b756c22c8f398efdf0eb19ff1123e4178cc80ae94bab4d`。另有
1,000 seeds、1,000 萬事件模擬、四項隔離 mutation 及乾淨副本 CLI crash/restart
測試。具體命令、版本、數量、限制、首敗與原始輸出見
`artifacts/reports/engineering-stage2.json`、`artifacts/acceptance-v1/`、
`artifacts/audit/` 和 `artifacts/clean-build/engineering-20261110-01/result.json`。

**本次新增 `CLAUDE.md`、本文件與框架建議文件後，來源指紋改變；上述是文件新增前的歷史結果，
尚未對新增文件後的來源重跑 P00～P08。** 單純文件交接並不代表測試失敗，但不能
稱舊 receipt 是當前來源的通過紀錄。使用 `scripts/status.py` 核對最新嘗試與當前指紋。
如需當前版本的完整替代驗收，來源穩定後執行：

```bash
. "$HOME/.cargo/env"
.venv/bin/python scripts/revalidate_all.py
```

此命令會依序重跑 P00～P08，包含多次完整模擬與實際子程序測試，耗時且寫入
`artifacts/`。如果修改 checker，其 hash 改變，必須從 P00 起重新依序驗收；先前
receipt 不能充作新 checker 的前一階段。個別修改先依風險跑針對性測試，完整驗收
以 `acceptance/v1.json`、`scripts/verify_gate.py` 和實際 receipt 為準。需要新建
乾淨副本時見 `docs/ENGINEERING_HARDENING.md`；它仍共用主機 Rustup、Python 和
package caches，不是新 OS 或冷快取驗證。

Windows 原生 Python 曾執行 checker 的 7 案例：5 通過、2 因 UNC symlink 建立權限
而跳過；相同 symlink 測試在 WSL 執行通過。Rust runtime 的驗收目標是 WSL/Linux，
沒有宣稱 Windows runtime 已驗證。SIGKILL 也不等於任意硬體斷電。

## 建議的下一階段：先討論 V2

**優先候選：WAL header integrity。** 已重現 v1 record 中間的 `payload_len` 若遭任意
毀損成合法上限內但超過 EOF 的值，可能被判成 incomplete tail 而截斷。這與正常
process crash 的 append-prefix 假設不同，具體格式與實際重現見
`docs/STORAGE_FORMAT_V1.md`、`docs/ENGINEERING_HARDENING.md` 和
`artifacts/audit/wal-review-20261110/results.json`。

建議交給使用者審閱的設計應至少回答：

1. 威脅模型：正常中斷、部分寫入、隨機 header／payload 毀損各如何辨識；哪些情況
   應 fail closed，哪些可安全截尾。checksum 有限制，不保證辨識所有毀損。
2. 格式方案：版本識別、header 完整性、長度上限、sequence、payload checksum 與
   decode 次序。避免在驗證 header 之前依不可信長度分配記憶體或決定截尾。
3. v1 相容與遷移：開啟舊資料的策略、備份、遷移中斷後重啟、原子切換與 rollback；
   v1 的既有歧義無法因轉換自動得到可信認證。不得在未確認資料安全與相容性時
   原地改寫使用者資料。
4. 驗收矩陣：v1/v2 版本、每個 header 欄位及 payload 毀損、短尾／中間毀損、超界
   長度、重複／跳號、同步失敗、遷移各 crash 邊界與重複啟動；明列預期成功、拒絕
   或可恢復，並檢查不產生錯誤 ACK、資料遺失或未經確認的 destructive repair。
5. 保留既有 V1 替代驗收與原始證據；另定 V2 條件與測試，區分真實 process crash、
   故障注入與未實測的硬體斷電。先設計並和使用者確認方向，再開始受授權的實作。

**後續候選，尚未決定：** snapshot/compaction 與 session 資源成長，需先設計
last-included index/term、KV 與 dedup sessions 的一致快照、原子安裝、leader/follower
追趕和中斷回復；不能只刪 WAL。其他如 membership、TLS、sharding、multi-Raft、
Windows runtime 與生產部署目前都不是已承諾範圍。先依使用者學習目標、風險與實際
瓶頸排序，再個別決議範圍、相容性和驗收，不要一次擴大。

## 可直接給 Claude Code 的第一項任務

> 請先閱讀專案根目錄 `CLAUDE.md`、`AGENTS.md` 與 `docs/HANDOFF_CLAUDE.md`，
> 核對目前 Git 狀態、`scripts/status.py`、WAL v1 格式及工程強化證據。請以台灣
> 繁體中文提出 WAL header integrity 的 V2 設計選項、舊資料遷移取捨、失敗情境、
> 測試和驗收矩陣，明確標出已重現與待驗證的部分，先和我討論，不要直接實作。
> 相鄰 ai-general-framework 保持唯讀，不要 push、建立 PR 或部署。
