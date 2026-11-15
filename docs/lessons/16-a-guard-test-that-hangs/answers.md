# 第 9 節解答

**1.** 關鍵是「狀態查詢在空轉開始之前還是之後被處理」。S4 一次處理一個輸入，先處理寫入就進入空轉，狀態查詢永遠排不到，1 秒的檢查失敗；
S5 在同一輪先回答狀態查詢、最後才提出寫入，檢查先通過，空轉之後才開始，測試接著在 `worker.join()` 永遠等下去。

**2.** 確認寫入真的提出了、連線池真的被用到。拿掉它，測試就無法分辨「actor 很健康」和「寫入根本沒走到複製」：
後者也會讓兩次狀態查詢都得到回覆，測試又會悄悄失效（教學 07 的情況）。

**3.** 一、輸出全部存在記憶體裡，每秒約 50 MB，600 秒還沒到，記憶體就先用完了；二、逾時只殺直接的子行程 cargo，底下的測試執行檔會繼續跑、繼續輸出。

**4.** 被殺掉的執行沒有自己失敗，我們不知道測試若能跑完會不會抓到錯。把它算成偵測到，會把「測試卡住」誤當成「測試有效」，正好掩蓋了這篇的問題。

**練習：實際執行結果（2026-11-15）**

推論：第一次狀態查詢仍在同一輪被回答，1 秒的檢查通過；接著 actor 空轉，讀不到 `Stop`，2 秒的限時等待觸發失敗。

在目前程式碼的副本中刪掉第二次查詢（從 `let (again_tx, again_rx)` 到 `tx.send(Input::Stop)` 之前），植入 `saturated_pool_spin`，經 `capped_run.py`（60 秒、256 MiB）執行：

```text
exit 101
thread 'tests::saturated_outbound_pool_does_not_starve_status' panicked at crates/raft-node/src/main.rs:1208:13:
the actor did not stop
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 5 filtered out
```

和推論一致：讓測試結束的是限時等待那一行 `assert!(Instant::now() < deadline, "the actor did not stop")`。
只靠限時等待也抓得到這個 mutant，但失敗訊息說的是「actor 沒停下」，沒說出真正的問題「狀態查詢被餓死」。第二次查詢讓測試在對的時間點、用對的理由失敗。
