# 接手文件

更新日期：2026-11-15。這份文件取代 2026-11-14 為 S1～S4 審查寫的版本（舊版在 Git 歷史中，`git show e5f09c2:docs/HANDOFF_CLAUDE.md`）。
它描述寫下時的狀態；接手時請先用下面的命令核對，**以實際查到的為準，不以這份文字為準**。

## 一開始先做什麼

```bash
cd /home/user/src/Distributed_Key-Value_Store_with_Raft
. "$HOME/.cargo/env"                    # 非登入的 shell 裡 cargo 不在 PATH 上
git status --short && git log --oneline -5 && git tag
.venv/bin/python scripts/status.py      # 每個階段最近一次驗收，以及是否對應目前的來源
uptime && ps -eo pid,pcpu,etime,comm --sort=-pcpu | head -5   # 機器是不是安靜的（見「踩過的坑」）
```

**`git tag` 有沒有 `s5-performance-verified` 是 S5 是否驗收通過的第一個線索**：這個 tag 只在整套 P00～P14 都是 `PASS [current source]` 之後才建立；沒有它，就是驗收還沒通過。
然後依任務讀：[路線圖](ROADMAP.md)（計畫與每階段的固定流程）、[AGENTS.md](../AGENTS.md)（不可違反的工程條件）、
[S5 獨立審查](INDEPENDENT_REVIEW_S5.md)（最近一次工作的來龍去脈與未結事項）、[S5 規格與補記](PERFORMANCE.md) §14、
[S1～S4 獨立審查](INDEPENDENT_REVIEW_S1_S4.md)。
`.venv`、`.agf`、`artifacts`、`target` 都被 `.gitignore` 排除；換機器或重新 clone 之後它們不存在，要先如實確認再往下做。

## 目標與目前的位置

專案的目的是**學習**與**面試時展示能力**，並藉開發過程改進使用者自己的 AI 框架（`/home/user/src/ai-general-framework`，唯讀）。
只維護一個最後版本；程式由 AI 完成，每個遇到的 bug 寫成新手導向的教學。最後整理成公開的 GitHub 儲存庫，push 由使用者自己做。

| 階段 | 狀態 |
|---|---|
| S0 基礎、S1 儲存格式、S2 快照與壓縮、S3 ReadIndex、S4 成員變更 | 已完成，各有 tag（`s0-…` 到 `s4-membership-verified`） |
| S1～S4 的獨立審查 | 2026-11-14 補做並修正（tag `s1-s4-review-verified`）；S1 只做到「不同 session」，還缺第三個模型 |
| S5 效能 | 實作完成（`claude-fable-5-2` 起頭、`claude-opus-5-5` 完成，規格 `PERFORMANCE.md`）；**獨立審查完成**（2026-11-15，`claude-sonnet-5-5`，[報告](INDEPENDENT_REVIEW_S5.md)）；審查找到的 F6、F7 已修正；F2～F5 **未處理**。驗收與 tag 以 `git tag`／`status.py` 為準 |
| **S6 整體強化** | **下一個階段**，開始前先寫規格並取得使用者同意；建議先處理 S5 的未結事項 |
| S7 公開整理 | 未開始 |

`ROADMAP.md`、`CLAUDE.md`、`PERFORMANCE.md` 檔頭仍寫「S5 已完成、tag `s5-performance-verified`」：那是作者在驗收開始前寫的，只有 tag 真的存在時才成立。

## 這個 session 做了什麼（2026-11-15）

1. 以不同於作者的模型（`claude-sonnet-5-5`）對 `e5f09c2` 做 S5 的獨立審查。沒有發現違反 Raft 安全性或 S5 保證的缺陷；找到 F1～F6（見報告 §1）。
2. 使用者要求把沒通過的部分在安靜的機器上重測。完整 mutation 執行三次都死在第 4 個 mutant（`saturated_pool_spin`），孤立重現後查明是 **F6**：
   S5 的 drain 迴圈讓守衛測試在該 mutant 下不再失敗而是卡住，而 `mutation_check.py` 把輸出全留在記憶體裡（約 52 MB/s），幾分鐘就撐爆 WSL。不是背景軟體造成的。
3. 使用者同意修正 F6：修測試、加 `scripts/capped_run.py` 限制單案例的時間與輸出、補契約與測試。
4. F6 修正後整套重驗一次：57/57 mutant 偵測到、模擬器 1,000/1,000、P00～P13 全過，P14 的所有測試也過，只在最後的報告檢查失敗——**F7**：契約要求 `write-benchmark.json` 的 `runs` 不得為空，
   但檢查器只認字串與物件，清單一律判錯，這條要求永遠過不了（P14 從沒跑完過）。
5. 使用者同意修正 F7：`verify_gate.py` 認清單（保留「不得為空」），補兩個測試。檢查器與契約都變，所以從 P00 全部重跑並重新產生報告；結果見 commit 訊息與 `scripts/status.py`。

兩個 commit：先是 F6、F7 的修正（`crates/`、`scripts/`、`acceptance/`，以及 `PERFORMANCE.md` §14.6 的更正），再是審查報告、本文件與框架經驗紀錄。

## 已決定的事與理由

| 決定（使用者 2026-11-15） | 理由 |
|---|---|
| 重測：先補跑，最後整套重驗 | `docs/` 計入來源指紋，文件定稿後 receipt 才會對應最終的檔案 |
| 整套驗收全過才打本機 tag `s5-performance-verified`，不 push | F1（驗收未完成）解決後，文件裡的「已完成、tag」才名實相符 |
| F2～F5 不修，只列進交接 | 審查者不改被審查的實作與測試；下一個 session 一次處理、只驗收一次 |
| F6 要修 | 它讓 P10 之後的驗收根本過不去，不修就無法驗收 |
| F7 要修，方式是讓檢查器認清單、保留「`runs` 不得為空」，不是把它從契約拿掉 | 前者保留原本的要求；後者是削弱驗收。改檢查器必須從 P00 全部重跑，使用者同意 |
| 審查報告 §1～§6、§8～§9 保留審查當時的原文，F6 與處理結果另寫（§7、§11） | 讓人分得出「當時看到什麼」與「後來改了什麼」 |

## 還沒做的事與未知

**S5 的未結事項（做法與要補的案例都在 [審查報告](INDEPENDENT_REVIEW_S5.md)，這裡只列摘要；建議一起處理，只驗收一次）**

- **F2（中）**：follower 接收端的批次上限（`crates/raft-core/src/lib.rs` 的 `entries.len() <= MAX_BATCH_ENTRIES`）沒有任何測試守著；改回 32，cargo 與 32 個整合測試都過，但落後超過 32 筆的 follower 永遠追不上。
  補 core 單元測試（256 筆成功、257 筆被拒）與真實 process 測試（`repro/test_zz_lag.py` 是現成的探針），並新增 mutation 案例。
- **F3（中低）**：`validate_delta` 有三條規則沒被測試獨立守住（entry 的 term 不得小於前一筆、命令必須合法、term 不得為 0）。在 `invalid_deltas_are_refused_without_writing` 補案例並各加一個 mutant。
- **F4（低）**：`PERFORMANCE.md` §14.5 的「log 成長」列與 `write-benchmark.json` 不符（正確：修正前 6.2 → 14.8 ms、修正後 5.1 → 5.5 ms）。
- **F5（低）**：一個恆真的斷言（`batches <= batched_writes`）；crash 測試不保證是整批；node 端 512 KiB 切批沒有測試；G6 測試只比 `save` 與 `save_delta`，建議存一個舊版產生的 golden WAL。
- **預檢（建議，未實作）**：`scripts/preflight_phase.py` 只跑階段自己的測試，不檢查報告的結構，所以 F7 要到整套驗收的最後一步才出現。讓它用現有報告預檢契約的報告檢查（不寫 receipt），可以省下一小時。
- **教學**：F6（測試失敗時卡住而不是失敗；擷取輸出沒有上限）、F7（契約與檢查器彼此不符，要到第一次真的跑才發現）與 F2、F3 都還沒寫成教學；下一篇編號是 16。依 AGENTS.md，遇到的 bug 要寫成教學。
- **無法解釋的失敗**：重測時有 4 次 `raft-node` 的 `an_oversize_state_is_not_serialized_again_until_the_log_doubles` 失敗，當時機器上有其他建置；之後單獨跑 50 次、整個檔案跑 30 次都沒有再失敗。原因不明，見報告 §8。

**其他**

- **S1 還缺一次由第三個模型做的審查。** S1 的規格草稿與第一個 commit 是 `claude-fable-5-2` 寫的；這次的審查者（`claude-sonnet-5-5`）只審了 S5，要審 S1 的話請另開 session，不要先給它結論。
- 留給 S6 的模擬器盲點：管理者一次只送一個成員變更；用戶端每個種子只有 64 個操作。審查時實測有 17 個植入錯誤只有單元測試抓得到（S1～S4 審查紀錄 §5）。
- 教學 01～15 的狀態都是「草稿」：沒有第二個人照著第 3 節做過。
- 沒有實測的事（主機斷電、真實叢集跑到 64 MiB 等）列在 `TEST_RECORD_20261114.md` §7。
- `docs/WORKLOG.md` 與 `CLAUDE.md` 的部分條目還停在 V1 時期的說法，沒有整理。

## 下一步

1. 核對 `git tag` 與 `status.py`。有 `s5-performance-verified` 且每一行都是 `PASS [current source]`：驗收成立。沒有：先看 commit 訊息說哪一步沒過；重跑整套之前先確認機器是安靜的。
2. 處理 F2～F5 與教學，**改完一次跑完整驗收**（`mutation_check.py`、`simulation_report.py`、`read_benchmark.py`、`write_benchmark.py`，再 `revalidate_all.py`；約 1 小時）。
3. （擇一）請第三個模型另開 session 審查 S1；或起草 S6 的規格給使用者看，先不要實作。
4. 每個階段完成時，在規格補記寫下**實作所用的模型**，下一次審查才判斷得出自己是不是「不同模型」。

## 操作範圍

- 修改只限本專案；`/home/user/src/ai-general-framework` 唯讀（`.claude/settings.json` 有 deny 規則）。
- 不 push、不建立 PR、不部署。本機 commit 與 tag 依使用者當次的指示。
- 不為了通過驗收而削弱測試或改契約；改既有測試的期待時，寫進契約的 `modified_tests` 並告訴使用者。
- 審查者不改被審查的實作；要改，先問使用者。
- 與框架、skill、宿主、交接或驗收工具有關的經驗，當下追記到 `docs/FRAMEWORK_EXPERIENCE_LOG.md`（目前到 L48）。
- 以自然的台灣繁體中文溝通；分清「已執行」「推論」「未驗證」。

## 這次踩過的坑（接手的人大概也會遇到）

- **長時間的工作會被打斷，而且連脫離 session 的背景工作也一起死。** 這個 WSL 在 session 結束時似乎會整個重啟（之後 `uptime` 只有 0～10 分鐘；原因是推論，現象有 `uptime` 與被殺的 `setsid` 工作為證）。
  `/tmp` 是 tmpfs，重啟就清空；要留的證據寫到 `artifacts/`（本次 `artifacts/audit/independent-review-20261115-s5/`），暫存目錄只放用完即丟的東西。
  驗收用 `scripts/run_phase.sh NN` 可以從某一階段續跑（需前一階段已有通過紀錄），`revalidate_all.py` 則從頭開始。
- **擷取輸出一定要有上限。** 失控的 mutant 或測試可以每秒噴 50 MB；`mutation_check.py` 現在經 `scripts/capped_run.py` 執行每個案例（480 秒、256 MiB）。自己寫腳本時同樣不要 `capture_output=True` 後讓它無限成長。
- **這個 WSL 開機就跑著 `dockerd` 和 `containerd`，常各佔約一顆 CPU**（load 8～18）。量測或驗收前先看 `uptime`，必要時先停掉它們；時序相關的測試在那種負載下不可靠。
- **報告與驗收執行期間不要改任何受指紋涵蓋的檔案**（`docs/` 也算）。報告帶著指紋，改了就得全部重跑。要寫文件就先寫完再跑。
  **改檢查器（`verify_gate.py`）或契約（`final.json`）也要從 P00 全部重跑**——receipt 帶著兩者的雜湊，前一階段的雜湊不符，下一階段就拒絕執行。
- **新階段的契約寫完後，立刻實際跑一次 `scripts/verify_gate.py NN`，不要等到階段收尾。** S5 的契約到驗收的最後一步才發現過不了（F7），而 S5 的作者在驗收開始前就把文件寫成「已完成」。
- **session 從 `/home/user/src` 啟動時，宿主不會列出專案的 skill**；這次在工作目錄進入專案目錄後宿主才補列，之後 `review-change` 能直接用 Skill 叫用。從專案根目錄啟動最省事，否則直接讀 `.claude/skills/<名稱>/SKILL.md` 照做。
- **非登入的 shell 找不到 `cargo`。** 先 `. "$HOME/.cargo/env"`，或把 `$HOME/.cargo/bin` 加進 `PATH`。
- **使用者環境的 RTK hook 會改寫 Bash 命令**，偶爾讓自動權限審查要求原樣重送；需要原始輸出時用 `rtk proxy <命令>`。
- **寫「逐一植入錯誤再建置」的腳本前先讀教學 06。** 共用建置目錄時 cargo 會沿用前一個案例的編譯結果。
- **改模擬器的工作負載或固定參數前先讀教學 11、12；改任何測試依賴的執行順序前先讀教學 07。** 「挑最合理的對象」、「固定的大小」和「順序」都曾讓整條路徑測不到——S5 的 F6 是同一類。
- `pkill -f <樣式>` 會連自己所在的 shell 一起殺掉（命令列裡有同樣的字）；用 PID。

## 重要材料的位置

| 要找什麼 | 在哪裡 |
|---|---|
| 計畫、階段、固定流程、模型分工 | `docs/ROADMAP.md` |
| 不可違反的工程條件 | `AGENTS.md` |
| 各階段的規格與實作補記 | `docs/STORAGE_FORMAT_V2.md`、`SNAPSHOT_COMPACTION.md`、`READ_INDEX.md`、`MEMBERSHIP.md`、`PERFORMANCE.md` |
| 驗收契約與說明 | `acceptance/final.json`、`docs/ACCEPTANCE_FINAL.md` |
| S5 審查的發現、證據與處理結果 | `docs/INDEPENDENT_REVIEW_S5.md` |
| S1～S4 審查的發現、證據與處理結果 | `docs/INDEPENDENT_REVIEW_S1_S4.md` |
| 測試紀錄（S1～S4 審查時） | `docs/TEST_RECORD_20261114.md` |
| bug 教學（15 篇）與寫法 | `docs/lessons/`、`.claude/skills/write-bug-lesson/SKILL.md` |
| 框架的使用經驗 | `docs/FRAMEWORK_EXPERIENCE_LOG.md` |
| 論文出處筆記 | `docs/reference/raft-notes.md` |
| receipt、報告、審查的重現材料（不在版本控制內） | `artifacts/acceptance-final/`、`artifacts/reports/`、`artifacts/audit/independent-review-20261115-s5/`（S1～S4：`…-20261114/`） |

## 可以直接交給下一個 session 的第一句話

> 請先讀 `CLAUDE.md` 與 `docs/HANDOFF_CLAUDE.md`，執行「一開始先做什麼」的命令並回報：工作樹有哪些未提交的變更、`git tag` 有沒有 `s5-performance-verified`、`status.py` 是否每一行都是 `PASS [current source]`、機器是否安靜。
> 然後（擇一）：(a) 依 `docs/INDEPENDENT_REVIEW_S5.md` 處理 F2～F5 與教學 16，一次改完再驗收一次；(b) 以不同於 `claude-fable-5-2` 的模型審查 S1，不要先讀審查紀錄的結論；
> (c) 起草 S6 的規格給我看，先不要實作。相鄰的 ai-general-framework 保持唯讀，不要 push、建立 PR 或部署。
