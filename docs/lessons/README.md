# Bug 教學

開發這個專案時實際遇到的 bug，每一個寫成一篇可以照著做的教學。
這些 bug 都是真的：有重現步驟、有當時的輸出、有修正前後的程式。

## 給第一次讀的人

只讀三篇的話，照這個順序：

1. [WAL 長度欄位毀損](01-wal-length-corruption/README.md)：一個壞掉的數字，讓節點忘記自己投過的票。從磁碟、checksum 這些看得到的東西開始，最容易進入。
2. [先 commit 的反而後套用](09-apply-overtaken-by-its-barrier/README.md)：一步順序排錯，狀態機就照錯的順序套用。先讀[帶讀](../LEARNING_GUIDE.md)的寫入流程。
3. [放寬了上限，卻沒有測試守著它](18-a-limit-nobody-tested/README.md)：所有測試都通過，系統卻有一種情況永遠卡住；看「測試通過」和「功能正確」差在哪裡。

## 依 bug 的種類

19 篇都是草稿：還沒有人照第 3 節實際做過一遍。「發現於」指寫成這篇教學的階段。

### 資料寫到磁碟與回復

需要的背景：WAL 與 checksum 是什麼、重啟後哪些狀態必須留住（[WAL 格式 2](../STORAGE_FORMAT_V2.md)）。

| 編號 | 題目 | 一句話 | 發現於 |
|---|---|---|---|
| 01 | [WAL 長度欄位毀損](01-wal-length-corruption/README.md) | 磁碟上一個數字壞掉，節點重開後默默刪掉 log，忘了自己投過票 | S1 |
| 11 | [快照裝到一半被終止，節點從此開不了機](11-install-from-a-newer-term/README.md) | follower 用一則訊息裝下 term 比自己新的快照，在兩個檔案之間被終止後，每次啟動都被拒絕 | S1～S4 的審查 |

### 共識的判斷時機與順序

需要的背景：commit 的規則（[commit 規則](../COMMIT_RULE_REVIEW.md)）與成員變更（[規格](../MEMBERSHIP.md)）；先讀[帶讀](../LEARNING_GUIDE.md)的寫入流程更好。

| 編號 | 題目 | 一句話 | 發現於 |
|---|---|---|---|
| 08 | [剛選出的 leader 把早就 commit 的設定當成「進行中」](08-judged-before-the-term-commit/README.md) | 新 leader 還不知道什麼已 commit 就先做判斷，每次換 leader 後成員變更都被誤拒 | S4 |
| 09 | [先 commit 的反而後套用](09-apply-overtaken-by-its-barrier/README.md) | 落盤 barrier 排在 Apply 前面，它完成時又 commit 了更新的 entry，69 比 68 先套用 | S4 |

### 卡住或變慢：進度與資源

需要的背景：快照與 log 壓縮（[規格](../SNAPSHOT_COMPACTION.md)）、批次上限（[效能規格](../PERFORMANCE.md)）。

| 編號 | 題目 | 一句話 | 發現於 |
|---|---|---|---|
| 03 | [壓縮開了，leader 卻沒東西可送](03-compaction-without-install-snapshot/README.md) | 只做 log 壓縮、還沒有 InstallSnapshot，第一個落後的 follower 就讓 leader 停住 | S2 |
| 13 | [資料超過快照上限之後，每做一件事都先白做一次大工程](13-oversize-snapshot-serialized-every-event/README.md) | 超過 64 MiB 後，節點每個事件都重新序列化整個狀態機再丟掉 | S1～S4 的審查 |
| 15 | [資料超過 1 MiB，節點就說不出自己的狀態](15-status-too-large-to-send/README.md) | 狀態查詢把整個 KV 放進回覆，超過訊框上限就送不出去，所有靠它找 leader 的工具一起失效 | S5 |
| 18 | [放寬了上限，卻沒有測試守著它](18-a-limit-nobody-tested/README.md) | follower 的批次上限改回 32，所有測試照過，落後超過 32 筆的 follower 卻永遠追不上 | S5 的審查 |

### 測試還是綠的，卻已經不測了

需要的背景：模擬器與 mutation 測試各自在驗什麼（[驗收契約](../ACCEPTANCE_FINAL.md)）。

| 編號 | 題目 | 一句話 | 發現於 |
|---|---|---|---|
| 07 | [測試還是綠的，卻已經不測了](07-test-silently-stopped-testing/README.md) | S3 讓 GET 不再複製，一個用 GET 製造複製工作的回歸測試悄悄失效 | S3 |
| 12 | [一千萬個事件全部通過，因為模擬器不再去問該問的節點](12-simulator-stopped-asking-stale-leaders/README.md) | S4 把模擬的請求一律送給最新的 leader，讀取的不變量從此碰不到它要抓的情況 | S1～S4 的審查 |
| 16 | [植入錯誤之後，守衛測試不失敗，而是永遠等下去](16-a-guard-test-that-hangs/README.md) | group commit 改了同一輪的處理順序，守衛測試在它要抓的錯誤下卡住；工具又把每秒 50 MB 的輸出全留在記憶體裡 | S5 的審查 |
| 19 | [測試案例被別的規則先攔下，它要守的規則就沒人守](19-a-case-stopped-by-another-rule/README.md) | 案例同時違反兩條規則，刪掉其中一條，另一條照樣擋下，測試看不出差別 | S5 的審查 |

### 測試工具與驗收本身出錯

需要的背景：同上一組；這幾篇的 bug 不在 KV store 裡，而在檢查它的工具裡。

| 編號 | 題目 | 一句話 | 發現於 |
|---|---|---|---|
| 02 | [測試偶爾說目錄已被佔用](02-flaky-lock-inherited-by-child/README.md) | 別的測試 fork 出的子程序，暫時帶走了這個測試的檔案鎖 | S2 |
| 04 | [檢查測試的工具也會說錯話](04-mutation-checker-false-survivors/README.md) | mutation 工具把「已失敗」與「沒執行」的測試報成「存活」 | S2 |
| 05 | [檔案存在不代表內容寫好了](05-marker-seen-before-written/README.md) | 測試看到中斷點的標記檔就去讀，讀到空字串；從 V1 就有的潛在問題 | S2 |
| 06 | [原始碼寫 2，執行的卻是 3](06-mutants-sharing-a-build-directory/README.md) | mutation 案例共用 cargo 建置目錄，後面的案例悄悄執行前面案例植入錯誤的程式 | S3 |
| 10 | [植入的錯誤存活了，但測試沒有問題](10-mutant-that-changed-nothing/README.md) | 一個 mutation 案例根本沒有改變程式行為（等價突變），被誤報成測試抓不到 | S4 |
| 14 | [量測工具量到的是它自己](14-the-benchmark-measured-itself/README.md) | 測試工具每送一個請求就重寫整份歷史檔，讓效能基準多出一個「來源不明、隨資料成長」的成本 | S5 |
| 17 | [驗收契約寫了一條檢查器永遠做不到的要求](17-a-rule-the-checker-could-not-meet/README.md) | 契約要求清單不得為空，檢查器卻只把它當字串檢查，再好的報告也過不了 | S5 的審查 |

## 怎麼讀

每篇都是同樣的十節。時間不多的話：讀第 1 節知道發生什麼事，跑第 3 節親眼看到，
再讀第 10 節看怎麼講。想真的弄懂，就照順序讀，並做第 9 節的練習。

## 寫作規則

樣板在 [TEMPLATE.md](TEMPLATE.md)。資料夾命名 `NN-簡短英文名稱`，內含：

| 檔案 | 內容 |
|---|---|
| `README.md` | 教學本文，十節齊全、順序固定 |
| `repro.sh` | 一行命令重現 bug，可重複執行，只在暫存目錄寫入 |
| `expected-output.txt` | `repro.sh` 實際執行的完整輸出 |
| `answers.md` | 第 9 節的解答 |
| 圖 | 以 Mermaid 圖源寫在 `README.md` 的 ```` ```mermaid ```` 區塊內，GitHub 會直接顯示；需要圖檔時再用工具渲染，不手寫 SVG |
| 其他輔助檔 | 例如只在舊版程式上執行的測試；在 `README.md` 說明用途 |

寫的時候請守住這幾點：

1. **只寫發生過的事。** 命令實際跑過，輸出取自實際結果，日期與 commit 可查。
   不確定當時的原因就寫不確定，不補編。
2. **新手讀得懂。** 每個術語第一次出現時用白話解釋並附原文；一段只講一件事；
   先講讀者會看到什麼，再講為什麼。
3. **用實際數值。** 例如寫「長度從 91 被改成 65536，檔案只有 230 bytes」，不寫「長度超出範圍」。
4. **分清楚程度。** 已執行驗證的、由程式推論的、還沒驗證的，各自標明。
5. **不誇大。** 是可用性問題就不說成資料遺失；修正擋不住的情況寫進第 8 節。
6. 草稿可以留「待補」，但狀態必須標「草稿」；標成「完成」前，`scripts/check_lessons.py` 必須通過，
   並且至少有一個人照著第 3 節實際做過一遍。

`scripts/check_lessons.py` 只檢查結構（章節、腳本、輸出檔是否存在），不判斷內容寫得好不好。
