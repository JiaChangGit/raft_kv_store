# 第 9 節解答

**1.** V1 的報告規則裡，`required_nonempty` 列的鍵只有 `command`（字串）、`metrics` 與 `environment`（物件），剛好都是舊檢查器認得的兩種型別。

**2.** 一條規則要有用，得分得出好壞：好的報告通過、壞的報告被拒絕。舊的檢查器對兩者給出相同的結果，表示它根本沒有在檢查 `runs` 的內容，問題在規則（檢查器怎麼解讀契約），不在報告。

**3.** 少了「報告真的有量測結果」這個保證。量測腳本若出錯、一筆都沒寫進 `runs`，驗收照樣通過，前後對照的數字卻不存在。

**4.** 報告帶著產生時的來源指紋，而驗收前一定會重新產生報告。開發中改了任何檔案，指紋就會不同，這是正常的；預檢要找的是「結構錯了、怎麼重新產生都過不了」的規則。

**練習：實際執行結果（2026-11-15）**

用 `e5f09c2` 的檢查器，對 `e5f09c2` 契約中每一條 `required_nonempty` 只檢查一個鍵，報告用 `artifacts/reports/` 裡現有的檔案（S5 最終驗收產生）：

```text
P07 verification.json command: met
P08 benchmark.json command: met
P08 benchmark.json metrics: met
P08 benchmark.json environment: met
...（P10～P14 的 command、isolation、environment 都是 met）
P14 write-benchmark.json runs: NEVER MET (list in the report)
```

只有一條：`write-benchmark.json` 的 `runs`，它在報告裡是清單。其他 20 個鍵都是字串或物件，舊的檢查器處理得了。
