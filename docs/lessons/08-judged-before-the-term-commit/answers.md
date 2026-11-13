# 第 9 節解答

**1.** `commit_index` 是「這個節點**知道**已 commit 到哪裡」，只存在記憶體裡。節點啟動時從快照位置開始（這裡沒有快照，所以是 0）。
三台都存有 index 1 確實代表它實際上已 commit，但 Raft 的 leader 只靠數副本來 commit **自己任期**的 entry；
之前任期的 entry，要等自己任期的某筆 entry commit 時才「間接」得知（博論 §3.6.2）。所以在 NoOp commit 之前，新 leader 的 `commit_index` 一直是 0。

**2.** 那就變成在舊任期未 commit 的設定還可能生效時，就允許附加新的設定。兩份設定可能同時「在競爭」，而它們可能相差兩台，多數不再重疊。
這正是 2015 年郵件反例的起點：舊 leader 留下的 D 和新 leader 的 E 各自有不重疊的多數，最後已 commit 的 E 被覆蓋。修正前的錯誤方向剛好相反（誤拒），所以只是可用性問題；反方向的錯誤則是安全性問題。（這一題是推論，依據是郵件的反例與 S4 的反例測試。）

**3.** 本任期的 entry commit 時，它之前的所有 entry 都同時被間接 commit（Log Matching）。所以那一刻起，`commit_index` 至少涵蓋了所有之前任期的 entry。
之後 log 裡還可能未 commit 的設定，只能是這個 leader 在本任期附加的（一個 term 只有一個 leader）。這時 `index > commit_index` 才和「尚未 commit」等價。

**4.** 可用性。依據：它只會讓一個本來安全的請求被拒絕，不會讓任何不安全的變更發生，也不會讓已 commit 的資料被改寫。

**練習：實際執行結果（2026-11-13）**

在兩個版本（commit `62e61d1`，以及把檢查順序還原的副本）各加入同一個測試變形：先讓 NoOp 的確認回到 leader，再送出移除節點 3 的請求。兩個版本的輸出相同：

```text
answers= commit_index before the request: 2
answers= right after the request: []
answers= at the end: [(40, Ok(Voters(6)))]
test result: ok. 1 passed
```

（`Voters(6)` 是位元集合 `0b110`，也就是 `{1, 2}`。）送出請求時 `commit_index` 已經是 2，修正前的判斷 `1 > 2` 不成立，所以兩個版本都正常完成變更。
這說明錯誤只存在於「當選」到「本任期第一筆 entry commit」之間的那段時間；那段時間通常只有幾毫秒，所以只檢查最終結果、又會重試的測試很難發現它。
