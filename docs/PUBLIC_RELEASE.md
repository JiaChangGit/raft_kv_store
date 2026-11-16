# S9 規格：公開整理

狀態：**已核准**（2026-11-16，使用者選「核准，開始做」）。撰寫與實作：`claude-opus-5-5`。依據：[路線圖](ROADMAP.md)的 S9 列與「2026-11-15 的計畫修改」。

## 1. 目標

把儲存庫整理成可以公開、可以在面試時展示的樣子：第一次看到它的人，十分鐘內要能知道這是什麼、怎麼跑、做到哪裡、哪些沒有驗證，以及 AI 在其中做了什麼。
不改程式的行為。最後從 P00 完整重跑驗收。push 由你自己做。

## 2. 不做的事

- 不改 Rust 程式、測試、驗收契約與檢查器。
- 不動三份原始規格：`START_HERE_zh-TW.md`、`ALL_IN_ONE_SPEC.md`、`01_FEASIBILITY_REVIEW.md`。
- 不改寫紀錄類文件的內容：有日期的紀錄、經驗紀錄、審查報告、實驗的原始紀錄。過時的只在開頭加標示。
- 不 push、不建立 GitHub repository、不改 Git 歷史。
- 教學 01～19 維持「草稿」：要有人照著做過一遍，狀態才改。

## 3. 現況（2026-11-16 盤點）

- **README.md** 只有中文。寫的驗收階段只到 13（現在到 15）；接手指向 `WORKLOG.md`（現在是交接文件）；指向已被取代的「框架回饋」；有一行 `cd` 到本機絕對路徑；沒有架構圖、各階段摘要、沒驗證的事與授權。
- **本機絕對路徑**：受版本控制的檔案裡，家目錄的絕對路徑出現在 30 個檔：`framework-study/` 22 個（實驗的原始紀錄與當時的腳本）、`docs/` 5 個、`README.md`、`CLAUDE.md`、`.claude/settings.json`（deny 規則必須用絕對路徑）。另有 6 個檔帶暫存目錄的路徑。三份原始規格裡沒有。
- **授權**：沒有 LICENSE。
- **作者資訊與秘密**：commit 作者用的是 GitHub 的 noreply 信箱。用常見的金鑰樣式掃過工作樹，沒有命中；這台機器沒有安裝秘密掃描工具。
- **文件**：已標示為歷史或已被取代的有 4 份（`ACCEPTANCE_V1`、`STORAGE_FORMAT_V1`、`FRAMEWORK_FEEDBACK`、`FRAMEWORK_IMPROVEMENT_PROPOSALS`）。V1 時期寫成、還沒標示的有 `IMPLEMENTATION_REPORT`、`VERIFICATION_FINDINGS`、`ENGINEERING_HARDENING`、`WORKLOG`、`COMMIT_RULE_REVIEW`、`FAULT_SCHEDULE_FORMAT`，以及 `diagrams/` 的兩張圖。帶讀文件與契約文件在 S2～S4 已經補過差異。
- **驗收**：上一次完整驗收（S6）約 43 分鐘：mutation 13 分、模擬與 benchmark 2 分、P00～P15 約 28 分。它的執行腳本放在不受版本控制的 `artifacts/`，寫死了本機路徑。

## 4. 要做的事

### 4.1 README：英文一份、中文一份

- `README.md`（英文）與 `README.zh-TW.md`（台灣繁體中文）內容對應，開頭互相連結。
- 章節：這是什麼；各階段做了哪些功能；架構圖；快速開始（建置、啟動三台、client、成員變更）；驗證到哪裡、怎麼重跑；**沒有驗證的事**；文件地圖；AI 協作的摘要；授權。
- 「沒有驗證的事」照路線圖要求的三點寫，再加上其他已知限制：沒有長時間浸泡測試，真實 process 的線性一致性一次最多看 24 個操作；模擬器的管理者一次只送一個成員變更、每個種子 64 個用戶端操作；S1 沒有第三個模型審查；教學沒有真人照做過；SIGKILL 不等於斷電；沒有形式化證明。
- 指令都從專案根目錄執行，不寫絕對路徑。
- 不寫會變動的數字（測試個數、benchmark 結果）；這類數字指向 `scripts/status.py`、報告檔與 `PERFORMANCE.md`。理由見 4.8：驗收跑完之後不能再改文件。

### 4.2 架構圖

- 新增 `docs/ARCHITECTURE.md`，Mermaid 圖源直接寫在 Markdown 裡：
  - 元件圖：7 個 crate（`raft-protocol`、`raft-core`、`raft-sim`、`raft-storage`、`kv-state`、`raft-transport`、`raft-node`）與 `tools/` 的關係。
  - 一次寫入：client → leader 的 actor → WAL 的 group commit → AppendEntries → follower 落盤後回覆 → commit → 套用 → 回覆 client。
  - 一次讀取：ReadIndex 的多數確認與套用到 readIndex。
  - 成員變更：一次一台，先追趕再加入。
- README 放元件圖與寫入時序兩張。
- 每張圖都對照程式核對（用 `trace-code`），寫下對照的檔案與函式。
- `mmdc` 在這台 WSL 無法渲染（路線圖已記），所以本機只能檢查圖源；GitHub 上的顯示要 push 後由你確認。
- `diagrams/` 的兩張 V1 圖：對照現在的程式，仍正確就保留並註明，不正確就標成 V1 的歷史圖，由新圖取代。

### 4.3 教學索引

- 重寫 `docs/lessons/README.md` 的目錄：依主題分組（儲存與回復、快照、ReadIndex、成員變更、效能、測試工具與驗收），加上給新手的建議閱讀順序，以及每篇需要的先備知識與對應的階段。
- 各篇內容不改；狀態照實維持「草稿」。`scripts/check_lessons.py` 要照樣通過。

### 4.4 本機絕對路徑

- **會繼續被人讀的文件**（README、`CLAUDE.md`、`AGENTS.md`、交接文件、`HANDS_ON.md`、各階段規格、帶讀）：改成相對於專案根目錄的寫法。
- **紀錄**（經驗紀錄、`WORKLOG.md`、`TEST_RECORD_20261114.md`、審查報告、`framework-study/` 的原始結果與輸出、實驗當時的腳本）：不改寫，在 `framework-study/README.md` 與文件地圖寫明「原始紀錄保留作者機器上的路徑」。
- **`.claude/settings.json`**：deny 規則要絕對路徑才有效，保留；README 說明這是作者本機的設定。
- 完成後用 `git grep -F "$HOME"` 列出剩下的家目錄路徑，逐筆對照是不是上面「不改寫」的類別。

### 4.5 文件地圖與過時文件的標示

- 新增 `docs/README.md`：所有文件分成「現行說明」「規格與補記」「紀錄」「歷史」四類，各一句話說明。
- 第 3 節列的 V1 文件逐份讀過，再在開頭加一行標示：寫成的時期，以及現在以哪份為準。仍然正確的（例如 commit 規則的推導）標「V1 時期撰寫，內容仍適用」；部分過時的，指出是哪幾節。

### 4.6 AI 協作的說明

- 新增 `docs/AI_COLLABORATION.md`（中文；README 放英文摘要）。內容：
  - 分工：你做決定、核准規格、抽查；AI 寫規格、程式、測試、教學與報告。
  - 每階段的固定流程，以及各階段用了哪個模型（取自規格補記）。
  - 三次獨立審查（S1～S4、S5、S8 後續）找到什麼。
  - bug 教學從哪裡來；框架怎麼被使用與分析（摘要 S8 報告與 S8 後續的結果）。
  - 這個做法的限制：寫的和評的常是同一個模型、實驗每格只跑一兩次、教學沒有真人照做過、專案的 deny 規則曾經整段沒有生效。
- 動筆前先讀 writing 指引。

### 4.7 授權、公開範圍與 push 前的檢查

- **LICENSE**：等你選（第 6 節）。
- **公開範圍**：`artifacts/` 維持不進版本控制；`framework-study/` 等你決定（第 6 節）。
- **push 前的檢查**（結果寫進本文件的補記）：工作樹與整個 Git 歷史用常見的金鑰樣式各掃一次；commit 作者資訊；超過 1 MB 的檔案；`.gitignore` 是否涵蓋 `artifacts/`、`data/`、`.agf/`、`.venv/`、`target/`。這台機器沒有秘密掃描工具，裝不裝由你決定；沒裝就照實寫「只做了樣式比對」。

### 4.8 完整重跑驗收

- 把 S6 的 `final_run.sh` 改成不寫死路徑的 `scripts/final_acceptance.sh`：可以續跑、開始時記下來源指紋、來源一變就停。README 寫出用法。
- **順序很重要**：所有文件改完、commit 之後才開跑，跑的期間不改任何受指紋涵蓋的檔案。交接文件與 README 事先寫成「tag 只在驗收通過後建立」，跑完不再改文件，讓 tag 指向的 commit 正好是驗收過的來源。
- 全部是 `PASS [current source]`，才建 tag `s9-public-verified`。有失敗就照實記錄，修好後從頭再跑。
- 預估約 45 分鐘，都是機器時間。

## 5. 驗收條件

- 兩份 README 涵蓋 4.1 的章節。快速開始的指令，在一份乾淨的 clone 裡照著做一遍：建置、啟動三台、PUT／GET、加入節點。
- `git grep -F "$HOME"` 只剩 4.4 列為「不改寫」的檔。
- 每份文件都在 `docs/README.md` 有分類；第 3 節列的 V1 文件都有標示。
- 每張架構圖都寫了對照的程式位置。
- `scripts/check_lessons.py` 通過；P00～P15 全部 `PASS [current source]`；tag 已建立。
- push 前檢查的結果寫進補記。

## 6. 要你決定的

**使用者的決定（2026-11-16）：MIT；英文為主；`framework-study/` 整個公開。** 下面保留當時的選項與說明。

1. **授權條款**：MIT（和你的框架一樣；我建議這個）、Apache-2.0，或先不加。不加的話，別人依法不能使用這些程式碼。
2. **README 的語言**：英文為主、另有一份中文（我建議這個，面試與 GitHub 的讀者多半先看英文），或中文為主、另有一份英文。
3. **`framework-study/` 公開到什麼程度**：整個公開（我建議這個：分析框架是專案的目的之一，原始紀錄讓結論可以被檢查），或只公開報告、不公開實驗的原始紀錄。

## 7. 模型分工

| 工作 | 由誰做 | 為什麼 |
|---|---|---|
| README、架構說明、AI 協作說明的文字；架構圖對照程式 | Opus 5.5（我） | 要整合所有階段的脈絡 |
| 路徑整理、文件標示、索引 | 我 | 量不大；交給子代理要重新讀一遍脈絡，反而更貴 |
| 照著 README 在乾淨 clone 裡做一遍 | Sonnet 5.5 子代理 | 模擬第一次接觸的讀者；只給 README 與 clone，不給我的說明 |
| 中文 README 與 AI 協作說明的可讀性校閱 | Sonnet 5.5 子代理 | 換一個模型看；依 writing 指引與 S7 的檢查清單，只給成品 |

## 8. 費用與時間

- 不需要另外付費的 CLI session（這一階段沒有對照實驗）。
- 兩個子代理用這個對話的額度。
- 驗收重跑約 45 分鐘的機器時間。

## 9. 不保證

- GitHub 上 Mermaid 圖的顯示，要 push 後才看得到。
- 教學仍是草稿，沒有真人照做過。
- README 的指令只在這台 WSL 與乾淨的 clone 驗證過，其他作業系統沒有試過。
