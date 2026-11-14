# 接手文件

更新日期：2026-11-14。這份文件取代 2026-11-10 從 Codex 轉交時寫的版本（舊版在 Git 歷史中，`git show 513cd58:docs/HANDOFF_CLAUDE.md`）。
它描述寫下時的狀態；接手時請先用下面的命令核對，**以實際查到的為準，不以這份文字為準**。

## 一開始先做什麼

```bash
cd /home/user/src/Distributed_Key-Value_Store_with_Raft
. "$HOME/.cargo/env"                    # 非登入的 shell 裡 cargo 不在 PATH 上
git status --short && git log --oneline -3 && git tag
.venv/bin/python scripts/status.py      # 每個階段最近一次驗收，以及是否對應目前的來源
```

然後依任務讀：[路線圖](ROADMAP.md)（計畫與每階段的固定流程）、[AGENTS.md](../AGENTS.md)（不可違反的工程條件）、
[獨立審查紀錄](INDEPENDENT_REVIEW_S1_S4.md)（最近一次工作的來龍去脈）、[測試紀錄](TEST_RECORD_20261114.md)（跑過什麼、結果在哪）。
`.venv`、`.agf`、`artifacts`、`target` 都被 `.gitignore` 排除；換機器或重新 clone 之後它們不存在，要先如實確認再往下做。

## 目標與目前的位置

專案的目的是**學習**與**面試時展示能力**，並藉開發過程改進使用者自己的 AI 框架（`/home/user/src/ai-general-framework`，唯讀）。
只維護一個最後版本；程式由 AI 完成，每個遇到的 bug 寫成新手導向的教學。最後整理成公開的 GitHub 儲存庫，push 由使用者自己做。

| 階段 | 狀態 |
|---|---|
| S0 基礎、S1 儲存格式、S2 快照與壓縮、S3 ReadIndex、S4 成員變更 | 已完成，各有 tag（`s0-…` 到 `s4-membership-verified`） |
| S1～S4 的獨立審查 | 2026-11-14 補做並修正；S1 只做到「不同 session」（見下） |
| **S5 效能** | **下一個階段，尚未開始**。開始前先寫規格並取得使用者同意 |
| S6 整體強化、S7 公開整理 | 未開始 |

## 版本與未提交的變更

- `HEAD` 是 `513cd58`（tag `s4-membership-verified`）。**獨立審查之後的所有修正都還在工作樹，沒有 commit，也沒有新的 tag。**
  使用者還沒有指示 commit；這個 session 的宿主規則是「使用者要求才 commit」。
- 未提交的內容（`git status --short` 應該看到這些；多了或少了就先弄清楚原因）：

| 類別 | 檔案 |
|---|---|
| 產品程式 | `crates/raft-storage/src/lib.rs`（回復時採用快照的 term）、`crates/raft-node/src/main.rs`（超過上限後的重試間隔） |
| 模擬器 | `crates/raft-sim/src/cluster.rs`、`crates/raft-sim/src/lib.rs`、`crates/raft-sim/tests/invariants.rs` |
| 測試與測試工具 | `crates/raft-storage/src/tests.rs`、`crates/raft-storage/tests/crash_process.rs`、`crates/raft-storage/src/bin/snapshot_probe.rs`、`tests/integration/test_read_index.py`、`scripts/mutation_check.py` |
| 契約 | `acceptance/final.json` |
| 規格與說明 | `docs/SNAPSHOT_COMPACTION.md`、`docs/READ_INDEX.md`、`docs/MEMBERSHIP.md`、`docs/ROADMAP.md`、`CLAUDE.md`、`docs/FRAMEWORK_EXPERIENCE_LOG.md`、`docs/lessons/README.md`、本文件 |
| 新檔案 | `docs/INDEPENDENT_REVIEW_S1_S4.md`、`docs/TEST_RECORD_20261114.md`、`docs/lessons/11-…`、`12-…`、`13-…` 三個資料夾 |

- 驗收：修正完成時（指紋 `5209bbdf…390dc`）P00～P13 全部通過。之後加入測試紀錄並改寫本文件，指紋因此改變，同一個 session 又重跑了一次。
  **現況請看 `scripts/status.py`**：每一行都是 `PASS [current source]` 才代表 receipt 對應你眼前的檔案；出現 `source changed / not run` 就表示之後有人動過檔案。

## 這個 session 做了什麼

1. 以新的 session、不同的模型對 `513cd58` 做獨立審查。沒有發現違反 Raft 安全性的缺陷；找到三個缺陷與一組模擬器的盲點。
2. 使用者指示「修，照建議做」後，修正三個缺陷並補測試：
   - **F1**（S2，教學 11）：follower 用一則訊息安裝 term 比自己新的快照時，在「快照已落盤、WAL 未改寫」之間被終止，之後永遠無法啟動。
     改為重啟時採用快照的 term、清除投票。
   - **F2**（S4 影響 S3 的證據，教學 12）：模擬器把請求一律送給 term 最高的 leader，讀取的不變量 I8 因此抓不到「未經多數確認就回覆」。
     產品程式沒有錯；修正的是模擬器的工作負載。
   - **F3**（S2，教學 13）：狀態機超過 64 MiB 後，每個事件都重新序列化一次再丟掉。改為間隔加倍後才重試。
3. 寫了教學 11～13、更新三份規格與契約、把審查紀錄放進版本控制，並重跑完整驗收。

## 已決定的事與理由

| 決定 | 理由 |
|---|---|
| F1 在**回復時**修（採用快照的 term），不在 core 拆成兩個 barrier | core 的規則是一步最多一個 barrier；回復時提高 term 而不投票在 Raft 中是安全的。兩種做法的比較在教學 11 §6 |
| 模擬器除了隨機挑選，還**主動**對過期的 leader 發讀取 | 實測只做隨機挑選時，1,000 個種子只有 43 次請求送到過期 leader，植入的錯誤仍全部通過 |
| 模擬器的快照分塊隨種子變化（7、13、4096 bytes），follower 安裝途中 crash 的機率 1/10 | 固定 7 bytes 讓「一則訊息送完」的路徑從未執行；只改分塊時 F1 的巧合在 1,000 個種子裡一次都沒出現 |
| 新增兩個「必須由模擬器抓到」的 mutation 案例，以及兩個必須大於零的統計欄位 | 讓「模擬器變弱」在驗收時以失敗出現，而不是繼續綠燈 |
| 既有測試 `snapshot_and_wal_consistency_rules` 的最後一列改成相反的期待 | 它原本斷言 F1 的磁碟狀態「必須拒絕」，那正是缺陷本身。**這是唯一一處改了既有測試的期待**，已記入契約的 `modified_tests`，值得使用者親自看一眼 |
| 新測試加進既有的階段 11、12，不另開階段 | 獨立審查是各階段固定流程的一步，補上的測試屬於它所審查的階段；階段 14、15 保留給 S5、S6 |
| 審查報告的 §1～§7 保留審查當時的原文，處理結果另寫在 §8 | 讓人分得出「當時看到什麼」與「後來改了什麼」 |

## 還沒做的事與未知

- **是否 commit、要不要加 tag，等使用者決定。** 之前各階段的慣例是本機 commit 加 tag；這次的修正跨 S2～S4，沒有現成的 tag 名稱可沿用。
- **S1 還缺一次由第三個模型做的審查。** S1 的規格草稿與第一個 commit 和這次的審查者是同一個模型（`claude-fable-5-2`）。
- 留給 S6 的模擬器盲點：管理者一次只送一個成員變更（「同時兩個變更」「蓋過未 commit 的設定」不會出現）；用戶端每個種子只有 64 個操作。
  審查時實測有 17 個植入錯誤只有單元測試抓得到，清單在審查紀錄 §5。
- 審查紀錄的觀察 O1～O3 沒有處理：O1（每個事件複製整份 log，成本隨資料量成長）屬於 S5；O2、O3 是已聲明的取捨。
- 教學 01～13 的狀態都是「草稿」：沒有第二個人照著第 3 節做過。
- 沒有實測的事（主機斷電、真實叢集跑到 64 MiB 等）列在測試紀錄 §7。
- `docs/WORKLOG.md` 與 `CLAUDE.md` 的部分條目還停在 V1 時期的說法，這次沒有整理。

## 下一步

1. 使用者決定是否 commit 這批修正（以及 tag）。
2. 若要嚴格符合路線圖：請第三個模型另開 session 審查 S1（`docs/STORAGE_FORMAT_V2.md`、`crates/raft-storage`），不要先給它這次的結論。
3. 開始 S5 前先寫規格。S5 的規格應該回應 O1：把「快照大小」當成量測的變數，並決定每個事件複製整份 log、`Wal::save` 逐筆比對、回復時逐筆複製這三件事怎麼處理。
4. 之後每個階段完成時，在規格補記寫下**實作所用的模型**，下一次審查才判斷得出自己是不是「不同模型」。

## 操作範圍

- 修改只限本專案；`/home/user/src/ai-general-framework` 唯讀（`.claude/settings.json` 有 deny 規則）。
- 不 push、不建立 PR、不部署。本機 commit 與 tag 依使用者當次的指示。
- 不為了通過驗收而削弱測試或改契約；改既有測試的期待時，寫進契約的 `modified_tests` 並告訴使用者。
- 與框架、skill、宿主、交接或驗收工具有關的經驗，當下追記到 `docs/FRAMEWORK_EXPERIENCE_LOG.md`（目前到 L42）。
- 以自然的台灣繁體中文溝通；分清「已執行」「推論」「未驗證」。

## 這次踩過的坑（接手的人大概也會遇到）

- **session 從 `/home/user/src` 啟動時，宿主不會列出專案的 skill。** 直接讀 `.claude/skills/<名稱>/SKILL.md` 照做即可。從專案根目錄啟動比較省事。
- **非登入的 shell 找不到 `cargo`。** 先 `. "$HOME/.cargo/env"`，或把 `$HOME/.cargo/bin` 加進 `PATH`。
- **使用者環境的 RTK hook 會改寫 Bash 命令**，偶爾讓自動權限審查要求原樣重送；需要原始輸出時用 `rtk proxy <命令>`。
- **寫「逐一植入錯誤再建置」的腳本前先讀教學 06。** 複製出來的檔案保留舊的修改時間，共用建置目錄時 cargo 會沿用前一個案例的編譯結果。這次的臨時腳本就重蹈了一次。
- **報告與驗收執行期間不要改任何受指紋涵蓋的檔案**（`docs/` 也算）。三份報告帶著指紋，改了就得全部重跑（約 35 分鐘）。要寫文件就先寫完再跑。
- **改模擬器的工作負載或固定參數前先讀教學 11、12。** 「挑最合理的對象」和「固定的大小」都曾讓整條路徑測不到。
- `pkill -f <樣式>` 會連自己所在的 shell 一起殺掉（命令列裡有同樣的字）；用 PID。

## 重要材料的位置

| 要找什麼 | 在哪裡 |
|---|---|
| 計畫、階段、固定流程、模型分工 | `docs/ROADMAP.md` |
| 不可違反的工程條件 | `AGENTS.md` |
| 各階段的規格與實作補記 | `docs/STORAGE_FORMAT_V2.md`、`SNAPSHOT_COMPACTION.md`、`READ_INDEX.md`、`MEMBERSHIP.md` |
| 驗收契約與說明 | `acceptance/final.json`、`docs/ACCEPTANCE_FINAL.md` |
| 審查的發現、證據與處理結果 | `docs/INDEPENDENT_REVIEW_S1_S4.md` |
| 這次跑過的測試 | `docs/TEST_RECORD_20261114.md` |
| bug 教學（13 篇）與寫法 | `docs/lessons/`、`.claude/skills/write-bug-lesson/SKILL.md` |
| 框架的使用經驗 | `docs/FRAMEWORK_EXPERIENCE_LOG.md` |
| 論文出處筆記 | `docs/reference/raft-notes.md` |
| receipt、報告、審查的重現材料（不在版本控制內） | `artifacts/acceptance-final/`、`artifacts/reports/`、`artifacts/audit/independent-review-20261114/` |

## 可以直接交給下一個 session 的第一句話

> 請先讀 `CLAUDE.md` 與 `docs/HANDOFF_CLAUDE.md`，執行「一開始先做什麼」的四個命令並回報：工作樹有哪些未提交的變更、`status.py` 是否每一行都是 `PASS [current source]`。
> 然後（擇一）：(a) 依我的指示處理這批修正的 commit；(b) 以不同於 `claude-fable-5-2` 的模型審查 S1，不要先讀 `INDEPENDENT_REVIEW_S1_S4.md` 的結論；
> (c) 起草 S5 的規格給我看，先不要實作。相鄰的 ai-general-framework 保持唯讀，不要 push、建立 PR 或部署。
