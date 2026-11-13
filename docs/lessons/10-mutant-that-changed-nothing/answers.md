# 第 9 節解答

**1.** 因為它沒有改變程式的行為。`holders` 裡多了 leader，但 `is_quorum` 先和設定取交集，leader 不在設定裡就被丟掉，票數和沒植入時完全一樣。
行為不變，任何測試都看不出差別，所以和測試寫得好不好無關。

**2.** 先確認植入真的改變了行為：沿著程式看植入的那一行之後，值會不會被後面的邏輯抵銷；必要時用最小的情境手算一次（像第 4 節的表格）。
確定行為有改變、測試卻仍通過，才是測試需要加強。

**3.** 交集是程式真正的防線：它讓「不在設定中的節點永遠不算票」這件事集中在一個地方，不依賴每個呼叫者都記得只放成員進來。
為了讓測試工具的植入有效而削弱程式，等於拿真實的安全性去換報告上的一個「抓到」。

**4.** 植入 B 不再呼叫 `is_quorum`，直接用 `holders.len()` 加上「自己不在設定中且已落盤時的一票」去和多數比較，繞過了交集這一道。
它仍然沿用 `holders`（只含設定內的成員），所以只多算了 leader 自己那一票，正好是這條規則禁止的事。

**練習：實際執行結果（2026-11-13）**

在 commit `62e61d1` 的副本中把 `is_quorum` 的交集拿掉（植入 C），執行 `raft-protocol` 與 `raft-core` 的測試：

```text
raft-protocol: tests::majority_follows_the_configuration --- FAILED
               test result: FAILED. 9 passed; 1 failed
raft-core:     membership_tests::only_voters_of_the_configuration_count --- FAILED
               test result: FAILED. 40 passed; 1 failed
```

- `majority_follows_the_configuration` 斷言「設定以外的節點永遠不算進多數」（例如 `{1,2,3}` 遇到 `{4,5,6,7}` 不構成多數），拿掉交集後這一條立刻不成立。
- `only_voters_of_the_configuration_count` 讓候選人收到節點 4、5 的同意票，斷言它不會當選；拿掉交集後它以 `{1,4,5}` 三票當選。

其餘 40 個 core 測試照樣通過，包括 `removing_a_follower_and_then_the_leader`：沒有植入 A 時，被移除的 leader 本來就不在 `holders` 裡，拿掉交集也不會多出它那一票。

再把植入 A 和植入 C **同時**放進同一份副本，執行 `removing_a_follower_and_then_the_leader`：

```text
assertion `left == right` failed: its own copy does not count
  left: 4
 right: 3
test result: FAILED. 0 passed; 1 failed
```

兩道防線同時被破壞時，leader 被算進去，第 4 筆在節點 2 收到之前就 commit 了，測試立刻抓到。
單獨植入 A，這個測試照樣通過（A 是等價的）；單獨植入 C，這個測試也通過，但 C 會改變別的行為，由上面另外兩個測試抓到。兩個合在一起，leader 才會被算進去。
