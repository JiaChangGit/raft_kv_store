# 教學 20 的解答

**1. GET 和 PUT 幾乎同時到達，修正前的 GET 會不會成功？**

會，而且是跟著 PUT 一起完成。PUT 落盤後 `Core::persisted` 呼叫 `finish_reads`，掛著的讀取就在那一刻被回答。

2026-11-17 用修正前的版本（`s9-public-verified`）實測：一台的叢集先寫入 `x=A`，等 0.3 秒；送出 GET，0.5 秒後再送一筆 PUT `y=B`。
GET 在 0.51 秒回 `OK`、結果 `A`，時間正好落在 PUT 落盤之後。沒有那筆 PUT 時，GET 等滿 3 秒失敗（第 3 節）。
所以這個 bug 只在「讀取時沒有其他寫入」時出現，邊寫邊讀的測試看不出來。

**2. `needed` 在兩台、三台、五台時各是多少？**

`needed = majority − 1`（leader 是 voter，扣掉自己）：

| voter 數 | majority | needed |
|---|---|---|
| 1 | 1 | 0 |
| 2 | 2 | 1 |
| 3 | 2 | 1 |
| 5 | 3 | 2 |

只有一台時是 0；其他情況至少要一個 follower 確認，所以一定要等到回覆，回覆到了 `finish_reads` 就會被呼叫。

**3. leader 不在自己的成員設定裡時呢？**

`voters.contains(self.id)` 是 false，不扣掉自己：`needed = majority`。例如成員設定只剩節點 2、leader 是正在被移除的節點 1，`needed` 是 1，
要等節點 2 確認。這時一定有 follower 要回覆，不會發生這篇的問題。（leader 在成員設定 commit 後就退位，見 [成員變更規格](../../MEMBERSHIP.md) §9.3。）

**4. 為什麼補上的那一行不會讓三台的叢集提早回答？**

`start_read_round` 先把 `read_round` 加一，新的讀取記下這個新的輪次。follower 的確認存在 `acked_round`，
只有「在輪次加一之後才送出」的請求得到回覆時，`acked_round` 才會到達新的輪次。補上的那一行和輪次加一在同一步執行，
這時沒有任何 follower 的 `acked_round` 到達新輪次，`finish_reads` 算出的 `confirmed` 比讀取的輪次小，讀取留著不回答。
單元測試 `read_tests::replies_to_requests_sent_before_the_read_do_not_confirm_it` 守的就是這件事，修正後仍然通過。

**動手練習。** 修正後，GET 回 `OK` 的時間在 0.1 秒以內（第 3 節的實測是 0.0 秒）；修正前等 3 秒回 `TIMEOUT_UNKNOWN`。
如果你在修正前的版本也立刻成功，檢查是不是剛好有其他寫入同時落盤（第 1 題），或者已經用 `kv_admin.py add` 加入了其他節點。
