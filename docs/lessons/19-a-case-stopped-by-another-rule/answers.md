# 第 9 節解答

**1.** 想測「entry 的 term 不能小於前一筆」（位置 2 是 term 3，位置 3 卻是 term 2）。它還違反「同位置同 term 不同命令」：位置 3 原本是 term 2、命令 `"x"`，案例放的是 term 2、命令 `"z"`。

**2.** 有前一筆時，前一筆的 term 至少是 1，term 0 一定小於它，「不能小於前一筆」會先擋下。空的 log 上，前一筆的 term 被當成 0（快照基準的 term），`0 < 0` 不成立，只剩「term 不能是 0」會擋。

**3.** 這次剛好能：規則還在時訊息是 `invalid or decreasing persistent state`，刪掉後變成 `same index and term …`，斷言訊息就會失敗。
但不夠：其他七條規則共用同一句訊息，案例若同時違反其中兩條，刪掉一條時訊息不變，斷言訊息照樣通過。要靠「每個案例只違反一條規則」，或讓每條規則有自己的錯誤。

**4.** 例如 `good(4, vec![entry(4, 3, "y")])`：位置 4 是新的、接得上；term 3 不是 0、不小於前一筆的 2；命令合法；位置 4 原本沒有 entry；只有「不能大於目前的 term 2」會擋。
（S5 的案例表裡，「entry term above the current term」正是這個案例，審查逐條移除時它被正確偵測到。）

**練習：實際執行結果（2026-11-15）**

推論：S5 的 `term 0 entry` 案例在位置 4，前一筆（位置 3）是 term 2，所以刪掉 `entry.term == 0` 之後，`entry.term < previous` 仍會擋下它，S5 的測試會通過；
目前的程式多了空 log 上的案例，前一筆的 term 是 0，沒有別的規則會擋，測試會失敗。

照 `repro.sh` 的做法取出兩份原始碼，各刪掉 `\n            || entry.term == 0`，執行 `cargo test -p raft-storage invalid_deltas_are_refused_without_writing`：

```text
S5（s5-performance-verified）: passed
目前的程式: FAILED: a term-0 first entry
```

和推論一致：S5 的案例是被「小於前一筆」擋下的；兩條規則共用同一句訊息，所以光看錯誤訊息也分不出來。
