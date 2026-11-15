# 接手文件

更新日期：2026-11-15（S6 收尾之後）。這份文件取代 S5 審查者寫的版本（舊版在 Git 歷史中：`git show 54319e6:docs/HANDOFF_CLAUDE.md`）。
它描述寫下時的狀態；接手時請先用下面的命令核對，**以實際查到的為準，不以這份文字為準**。

## 一開始先做什麼

```bash
cd /home/user/src/Distributed_Key-Value_Store_with_Raft
. "$HOME/.cargo/env"                    # 非登入的 shell 裡 cargo 不在 PATH 上
git status --short && git log --oneline -5 && git tag
.venv/bin/python scripts/status.py      # 每個階段最近一次驗收，以及是否對應目前的來源
uptime && ps -eo pid,pcpu,etime,comm --sort=-pcpu | head -5   # 機器是不是安靜的（見「踩過的坑」）
```

**`git tag` 有沒有 `s6-review-fixes-verified`，是 S6 是否驗收通過的第一個線索**：這個 tag 只在 P00～P15 都是 `PASS [current source]` 之後才建立；沒有它，就是驗收還沒通過，
先看最後一個 commit 的訊息說哪一步沒過。S7 起會加入不屬於 Raft 的檔案，`status.py` 顯示 `source changed` 是預期中的事（路線圖「2026-11-15 的計畫修改」）。
然後依任務讀：[路線圖](ROADMAP.md)（計畫、每階段的固定流程、S7 與 S8 的依據）、[AGENTS.md](../AGENTS.md)（不可違反的工程條件）、
[S6 的規格與補記](PERFORMANCE.md) §15、[S5 獨立審查](INDEPENDENT_REVIEW_S5.md)。
`.venv`、`.agf`、`artifacts`、`target` 都被 `.gitignore` 排除；換機器或重新 clone 之後它們不存在，要先如實確認再往下做。

## 目標與目前的位置

專案的目的是**學習**，以及**分析使用者自己的 AI 框架**（`/home/user/src/ai-general-framework`，唯讀）的優缺點、提出未來的優化建議；也要能在面試時展示。
只維護一個最後版本；程式由 AI 完成，每個遇到的 bug 寫成新手導向的教學。最後整理成公開的 GitHub 儲存庫，push 由使用者自己做。

2026-11-15 使用者縮減了計畫：只做做得出來的事和重點功能（[路線圖](ROADMAP.md)「2026-11-15 的計畫修改」）。

| 階段 | 狀態 |
|---|---|
| S0 基礎～S5 效能 | 已完成，各有 tag（`s0-…` 到 `s5-performance-verified`）；S1～S4、S5 各做過一次獨立審查 |
| **S6 收尾（縮減）** | 實作與教學完成（`claude-opus-5-5`）：S5 審查的 F2～F5、預檢報告結構、教學 16～19、契約階段 15。驗收與 tag 以 `git tag`／`status.py` 為準 |
| **S7 中文 AI 味檢查** | **下一個階段**，未開始。先寫規格給使用者同意 |
| S8 框架分析與對照實驗 | 未開始 |
| S9 公開整理（完整版） | 未開始；最後從 P00 完整重跑驗收 |

## 這個 session 做了什麼（2026-11-15）

1. 讀交接與 S5 審查後先問使用者四個問題，得到四個決定（下表），改寫路線圖並提交（`f7d226b`）。
2. S6：補 S5 審查的 F2～F5。F2：follower 的批次上限有了 core 單元測試（256 接受、257 丟棄）、真實 process 測試與兩個 mutation 案例。
   F3：`validate_delta` 的三條規則各有只違反它的案例與 mutation 案例。F4：`PERFORMANCE.md` §14.5 改成 S5 最終驗收那份報告的數字。
   F5：拿掉恆真的斷言；新 failpoint `after_sync_of_a_batch_before_ack` 與新的 crash 測試保證整批；node 端切批的單元測試；舊版程式寫出的 golden WAL。
3. `scripts/preflight_phase.py` 會用磁碟上的報告預檢契約的報告規則（F7 那類錯誤幾秒就出現）。
4. 教學 16～19（F6、F7、F2、F3），重現與練習都實際跑過。框架經驗紀錄 L49～L51。
5. 完整驗收：63 個 mutation 案例、模擬器、讀寫 benchmark、P00～P15。結果見 commit 訊息與 `status.py`。

## 已決定的事與理由

| 決定（使用者 2026-11-15） | 理由 |
|---|---|
| Raft 工程補完 F2～F5、驗收一次就收尾；不做長時間模擬、故障注入下的線性一致性、S1 的第三個模型審查 | 專案拖太久；只做做得出來的事和重點功能。沒驗證的事寫進 README |
| 公開整理做完整版（S9） | 面試展示要有架構圖、教學索引與 AI 協作說明 |
| 框架分析：報告＋2～3 個小型對照實驗（S8） | 主要目的之一是分析框架；只有報告沒有實驗，結論太依賴印象 |
| AI 味：實例分析＋檢查清單＋規則式腳本原型（S7） | 兩篇論文只研究英文，中文的特徵要自己定義、自己量誤報 |

S6 的實作決定（我做的，寫在 `PERFORMANCE.md` §15.3）：新測試放進新的**階段 15**，不塞進 S5 的階段 14（契約的成長規則）；原本的 crash 測試不動，另寫一個保證整批的；
審查 §8 的無法解釋的失敗不追查，驗收時再出現就照實記錄。

## 還沒做的事與未知

- **S7 的規格**：檢查清單的項目要對應使用者的七個特徵（原文在路線圖）；腳本是規則式的原型，要在本專案的文件上試跑、報告誤報率，只當提示。
  兩篇論文的摘要來自網頁讀取代理，引用前要回原文核對。
- **S8 的實驗設計**：候選是「skill 是否被正確觸發」「交接文件能否讓新 session 接手」等，評分要事先寫好，記錄模型、命令與原始輸出。證據來源：`FRAMEWORK_EXPERIENCE_LOG.md`（L01～L51）、兩份審查、框架原始碼。
- **教學 01～19 都是草稿**：沒有第二個人照著第 3 節做過。
- **審查 §8 那個無法解釋的失敗**（`an_oversize_state_is_not_serialized_again_until_the_log_doubles`）沒有追查。
- 因為計畫縮減而沒有驗證的事：沒有長時間浸泡測試；真實 process 的線性一致性一次只看 24 個操作；模擬器只檢查安全性、沒有活性（教學 18）；
  管理者一次只送一個成員變更、每個種子 64 個操作；S1 沒有第三個模型審查。主機斷電、真實叢集跑到 64 MiB 等列在 `TEST_RECORD_20261114.md` §7。
- `docs/WORKLOG.md` 與 `CLAUDE.md` 的部分條目還停在 V1 時期的說法，留給 S9 整理。

## 下一步

1. 核對 `git tag` 與 `status.py`。有 `s6-review-fixes-verified` 且 P00～P15 都是 `PASS [current source]`：S6 成立。
2. 起草 S7 的規格給使用者看，先不要實作。
3. 每個階段完成時，在規格補記寫下**實作所用的模型**。

## 操作範圍

- 修改只限本專案；`/home/user/src/ai-general-framework` 唯讀（`.claude/settings.json` 有 deny 規則，但見「踩過的坑」的第一條）。
- 不 push、不建立 PR、不部署。本機 commit 與 tag 依使用者當次的指示。
- 不為了通過驗收而削弱測試或改契約；改既有測試的期待時，寫進契約的 `modified_tests` 並告訴使用者。
- 與框架、skill、宿主、交接或驗收工具有關的經驗，當下追記到 `docs/FRAMEWORK_EXPERIENCE_LOG.md`（目前到 L51）。
- 以自然的台灣繁體中文溝通；分清「已執行」「推論」「未驗證」。

## 踩過的坑（接手的人大概也會遇到）

- **唯讀調查時不要 `cd` 進框架。** `cd` 會延續到之後的命令，宿主也會把主要工作目錄改成框架；session 從 `/home/user/src` 開始時，專案的 deny 規則是否生效沒有驗證過（L50）。
  用絕對路徑或子 shell `(cd 目錄 && 命令)`。
- **長時間的工作會被打斷。** 這個 WSL 在 session 結束時似乎會整個重啟（推論）；`/tmp` 是 tmpfs，重啟就清空，也吃記憶體。要留的證據寫到 `artifacts/`，大型建置放 `artifacts/runs/` 或 `target/`。
  驗收用 `scripts/run_phase.sh NN` 可以從某一階段續跑（需前一階段已有通過紀錄），`revalidate_all.py` 則從頭開始。
- **擷取輸出一定要有上限。** 失控的 mutant 每秒可以寫 50 MB；`mutation_check.py` 經 `scripts/capped_run.py` 執行每個案例（教學 16）。自己寫腳本時也一樣。
- **這個 WSL 開機就跑著 `dockerd` 和 `containerd`，常各佔約一顆 CPU。** 量測或驗收前先看 `uptime`。
- **報告與驗收執行期間，不要改任何受指紋涵蓋的檔案**（`docs/` 也算）。要寫文件就先寫完再跑。改檢查器（`verify_gate.py`）或契約（`final.json`）要從 P00 全部重跑。
  想在驗收進行中繼續工作：把來源複製到 `target/` 底下（不在指紋內）再跑，例如這次的 `target/s6-mutants/`；或只在暫存目錄寫草稿。
- **契約寫完立刻預檢。** `scripts/preflight_phase.py NN` 會跑該階段自己的測試，並用磁碟上的報告檢查到該階段為止的報告規則；舊報告缺新的 mutation 案例也會列成問題，重新產生後再跑一次（教學 17）。
- **改處理順序、放寬或收緊上限之前，先讀教學 07、12、16、18**：測試悄悄停止測試、卡住而不是失敗、只測了邊界的一側。寫「逐一植入錯誤再建置」的腳本前先讀教學 06。
- **專案 skill 的載入**：這次宿主把 `.claude/skills` 的 skill 列成「套用於專案目錄」；之前從 `/home/user/src` 啟動時曾經沒有列出（L47）。列不出來時，直接讀 `.claude/skills/<名稱>/SKILL.md` 照做。
- **RTK hook 會改寫 Bash 命令**，偶爾讓自動權限審查要求「原樣重送改寫後的命令」；照它給的命令重送即可。需要原始輸出時用 `rtk proxy <命令>`。
- **非登入的 shell 找不到 `cargo`。** 先 `. "$HOME/.cargo/env"`。
- `pkill -f <樣式>` 會連自己所在的 shell 一起殺掉；用 PID。

## 重要材料的位置

| 要找什麼 | 在哪裡 |
|---|---|
| 計畫、階段、固定流程、S7 與 S8 的依據（使用者原文） | `docs/ROADMAP.md` |
| 不可違反的工程條件 | `AGENTS.md` |
| 各階段的規格與實作補記 | `docs/STORAGE_FORMAT_V2.md`、`SNAPSHOT_COMPACTION.md`、`READ_INDEX.md`、`MEMBERSHIP.md`、`PERFORMANCE.md`（S6 在 §15） |
| 驗收契約與說明 | `acceptance/final.json`（階段 00～08、10～15）、`docs/ACCEPTANCE_FINAL.md` |
| 審查的發現與處理 | `docs/INDEPENDENT_REVIEW_S5.md`、`docs/INDEPENDENT_REVIEW_S1_S4.md` |
| bug 教學（19 篇）與寫法 | `docs/lessons/`、`.claude/skills/write-bug-lesson/SKILL.md` |
| 框架的使用經驗與改進候選 | `docs/FRAMEWORK_EXPERIENCE_LOG.md`、`docs/FRAMEWORK_IMPROVEMENT_PROPOSALS.md`、`docs/FRAMEWORK_FEEDBACK.md` |
| 論文出處筆記 | `docs/reference/raft-notes.md` |
| golden WAL 與它的來歷 | `crates/raft-storage/testdata/` |
| receipt、報告、審查的重現材料（不在版本控制內） | `artifacts/acceptance-final/`、`artifacts/reports/`、`artifacts/benchmarks/`、`artifacts/audit/independent-review-20261115-s5/` |

## 可以直接交給下一個 session 的第一句話

> 請先讀 `CLAUDE.md` 與 `docs/HANDOFF_CLAUDE.md`，執行「一開始先做什麼」的命令並回報：工作樹有哪些未提交的變更、`git tag` 有沒有 `s6-review-fixes-verified`、
> `status.py` 是否 P00～P15 都是 `PASS [current source]`、機器是否安靜。然後依 `docs/ROADMAP.md` 的「S7、S8 的依據」起草 S7（中文 AI 味檢查）的規格給我看，先不要實作。
> 相鄰的 ai-general-framework 保持唯讀，不要 push、建立 PR 或部署。
