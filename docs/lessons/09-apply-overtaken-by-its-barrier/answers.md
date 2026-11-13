# 第 9 節解答

**1.** 超車需要「barrier 完成時能再 commit 一筆新 entry」。barrier 完成只代表 leader **自己**的那份落盤了；
若設定裡還有其他投票成員，新 entry 還沒送出去，別人不可能已經有它，leader 一台湊不滿多數，就不會在 `persisted()` 裡產生新的 `Apply`。
只有在 leader 自己就是多數（設定只有它一台）時，才會在 barrier 完成的當下 commit 並套用。

**2.** `Apply` 處理的是這一步之前就已 commit 的 entry，已經在多數節點落盤、內容不會再改變，不依賴這次 barrier 寫入的任何狀態。
`Send` 則常常是在宣告這次 barrier 的內容：例如 follower 回覆「我收到 index 5 了」，或 candidate 送出投票請求。訊息若先送出、之後落盤失敗或 crash，
對方就會相信一件磁碟上不存在的事（例如重啟後忘了自己投過票）。所以 `Send` 必須在 barrier 之後。

**3.** 因為以前根本不可能發生（第 4 節）。固定三台時，leader 不可能只靠自己的落盤 commit 任何東西，barrier 的完成永遠不會產生 `Apply`。
S4 之前的測試不缺這個情境，是那時的系統沒有這個情境。S4 新增的單元測試是照我對流程的理解寫的，沒有組合出「等待中的變更＋commit 的那一步＋縮成一台」，模擬器的隨機工作負載才湊出來。

**4.** 能。若 host 把 `persisted()` 回傳的效果排到目前清單的**後面**，68 就會先套用。代價是：節點程式與模擬器兩個 host 都要改，
而且這變成一條 host 必須記住的隱性規則，將來任何新的 host（例如另一個測試工具）寫錯就會重新出現。修在 core 裡，所有 host 不需要改就正確。
（這一題是推論，沒有實作這個替代方案。）

**練習：實際執行結果（2026-11-13）**

在兩個版本（commit `62e61d1`，以及把 barrier 放回最前面的副本）各加入同一個變形：從 `{1,2,3}` 移除節點 3，等 NoOp 的確認到達的那一步。

```text
修正前：effect order: ["persist", "apply", "apply"]
        last applied on node 1: 2, answers: []
修正後：effect order: ["apply", "apply", "persist"]
        last applied on node 1: 2, answers: []
```

修正前的版本**仍然**把 barrier 排在最前面，但沒有發生超車：剩下 `{1,2}` 兩台時，barrier 完成只代表節點 1 自己落盤了 index 3，還缺節點 2，
所以 `persisted()` 沒有 commit 任何東西，也沒有產生 `Apply`；節點 1 依序套用到 2 就停住，變更還在等節點 2。
順序錯誤一直都在，只有在「barrier 完成就能 commit」時才會變成可見的超車。
