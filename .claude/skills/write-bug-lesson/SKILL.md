---
name: write-bug-lesson
description: Turn a bug found while developing this Raft KV store into a beginner-oriented lesson under docs/lessons, with a runnable reproduction and saved real output. Use when a defect has been reproduced and its root cause confirmed, or when asked to write or revise a bug lesson. Not for diagnosing an unexplained failure (find the root cause first), general concept teaching, or release notes.
---

# 把 bug 寫成教學

這是本專案自有的 skill，不由 ai-general-framework 管理。樣板與寫作規則的權威版本在
`docs/lessons/TEMPLATE.md` 與 `docs/lessons/README.md`，先讀那兩份；這裡只講流程。

## 動筆前要有的東西

缺任何一項就先補齊，不要邊寫邊猜：

- 可重複執行的重現步驟，以及實際輸出。
- 已確認的根因：能指出哪一行在哪個條件下做了錯的判斷。只有「改了就好了」不算。
- 首次失敗的紀錄位置（測試輸出、trace、資料檔）。
- 修正的 commit，以及修正前後各一次的測試結果。

根因還不清楚時，先依 `debug-root-cause` 的做法查清楚再回來。

## 流程

1. 在 `docs/lessons/` 取下一個編號，建立 `NN-簡短名稱/`，複製樣板為 `README.md`，狀態標「草稿」。
2. 先寫 `repro.sh`。它只在暫存目錄寫入、可重複執行、結束時印出一行明確的結論
   （例如 `BUG REPRODUCED: term went from 2 back to 1`）。需要舊版程式時，用
   `git archive <tag> | tar -x -C <暫存目錄>` 取得並在那裡建置（不會在儲存庫留下任何紀錄）。
   若要證明某個測試抓得到 bug，把測試檔複製進舊版副本執行，確認它在那裡失敗。
3. 實際執行，把完整輸出存成 `expected-output.txt`。之後正文引用的數值都從這個檔案來。
4. 依樣板寫第 1～10 節。第 4 節的圖用 Mermaid 圖源寫在 ```` ```mermaid ```` 區塊內（GitHub 會直接顯示），不手寫 SVG。
   引用論文前到 `docs/reference/raft-notes.md` 找出處並回原文核對句子。
5. 寫 `answers.md`。
6. 執行 `python3 -B scripts/check_lessons.py`，修到通過。
7. 以沒看過這個 bug 的讀者身分，從頭照第 3 節做一遍。卡住或需要回頭查的地方就是要改的地方。
8. 更新 `docs/lessons/README.md` 的目錄。請使用者實際照做後，再把狀態改為「完成」。

## 容易犯的錯

- 把推論寫成事實。沒有實際跑過的說法，要標「推論」。
- 第 2 節塞進整個 Raft 的介紹。只留這一篇需要的概念。
- 第 5 節誇大後果。可用性問題不要寫成資料遺失。
- 重現腳本依賴作者機器上的路徑或既有資料。
- 第 10 節變成條列功能。面試官想聽的是你怎麼發現、怎麼判斷、怎麼取捨。

## 回饋給框架

每寫完一篇，在 `docs/FRAMEWORK_EXPERIENCE_LOG.md` 記一條：這個流程哪一步有用、哪一步多餘、
哪裡缺了指引。累積幾篇之後，才評估是否值得提議成為框架的通用 skill。
