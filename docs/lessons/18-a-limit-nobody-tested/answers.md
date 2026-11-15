# 第 9 節解答

**1.** 一、收的一方沒有任何測試餵超過 32 筆的請求，送的一方的測試只檢查 leader 怎麼切批。
二、模擬器的不變量全是安全性，follower 拒絕請求不違反任何一條；整合測試裡的落後追趕案例走快照，不走 log。

**2.** core 處理 AppendEntries 時，不合規定的請求在 `reset_election` 之前就 `return`。follower 的選舉計時器因此從不重設，一到期就自己當 candidate、term 加一；
它下一次的失敗回覆帶著更高的 term，而 `fn message` 開頭的 `if term > self.state.term { self.step_down(...) }` 讓 leader 退位，重新選舉。反覆下去，term 一路上升。

**3.** 256 守住「不能比 256 嚴」（上限被改小，例如改回 32，就會失敗）；257 守住「不能比 256 鬆」（上限被拿掉或放大，就會接受 257 筆而失敗）。
只測 256，有人把上限整個拿掉時測試照樣通過。

**4.** 活性：沒有發生「壞事」（沒有錯誤的 commit），而是「好事永遠不發生」（follower 永遠追不上）。模擬器的 I1～I10 只檢查壞事有沒有發生，所以看不到。

**練習：實際執行結果（2026-11-15）**

推論：叢集建立時 log 已有 4 筆（設定、no-op、兩次加入節點）。停機期間寫入 20 筆，node 2 缺 20 筆，一個請求帶 20 筆，不超過 32，會被接受；
寫入 40 筆，缺 40 筆，超過 32，每次都被拒絕。

用 `repro.sh` 同樣的方法建出「S5＋上限 32」的節點程式，以 `probe.py <節點程式> <暫存目錄> <標題> 20` 與 `… 40` 執行：

```text
== limit 32, 20 writes while stopped
   node 2 stopped, 20 writes committed (last_applied 24, snapshot_index 0); node 2 started again
   node 2 caught up in 0.3 s

== limit 32, 40 writes while stopped
   node 2 stopped, 40 writes committed (last_applied 44, snapshot_index 0); node 2 started again
   node 2 NOT caught up after 15 s: role Candidate, term 32, last_applied 0
   node 1: role Follower, term 31, entries sent in AppendEntries 57424
   node 3: role Follower, term 32, entries sent in AppendEntries 176938
```

和推論一致：`last_applied 24` 減去 20 筆寫入，證實叢集建立時已有 4 筆。落後 20 筆時 0.3 秒追上，落後 40 筆時永遠追不上，term 也一路升到 32。
所以門檻就是「缺的筆數是否超過 32」；也因此只有落後較多的 follower 才會碰到，平常的測試很難剛好遇上。
