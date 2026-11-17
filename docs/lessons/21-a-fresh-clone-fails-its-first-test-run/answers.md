# 教學 21 的解答

以下的實測都在 2026-11-17，用 tag `s9-public-verified`（修正前）的副本執行。

**1. 為什麼第一次可能失敗，第二次就不會？**

第一次跑時 `target/node-pool-tests` 還不存在，三個測試同時要建立它，晚到的會拿到 `AlreadyExists`。
測試結束後這一層目錄留著，第二次跑時 `create_directory` 一開始的 `path.is_dir()` 就回傳成功，不會走到 `create_dir`。

實測：刪掉 `target/node-pool-tests` 後第一次跑 `cargo test -p raft-node`，`3 passed; 3 failed`；緊接著第二次，`6 passed; 0 failed`。

**2. 依序執行（`--test-threads=1`）時，修正前還會失敗嗎？**

不會。一次只有一個測試在建目錄，沒有人和它競爭：第一個測試建好上層，後面的測試看到它已經存在。

實測：刪掉 `target/node-pool-tests` 後用 `cargo test -p raft-node -- --test-threads=1`，`6 passed; 0 failed`。
這也是競爭的典型特徵：改變執行的先後或平行程度，結果就跟著變。

**3. 為什麼不能看到 `AlreadyExists` 就一律當成功？**

`AlreadyExists` 只說「那個名字已經被佔用」，佔用它的可能是一個**檔案**。例如有人把資料目錄的路徑誤設成一個既有的檔案，
那時應該回報錯誤、拒絕啟動，而不是把檔案當成目錄繼續做下去。所以修正同時要求 `path.is_dir()`。

**4. 為什麼不用 `fs::create_dir_all`？**

`create_dir_all` 確實會把「別人剛建好」當成功，但它只建立目錄，不對上層做 `fsync`。這個專案要求建立的目錄在當機後仍然存在
（[WAL 格式 2](../../STORAGE_FORMAT_V2.md) 的初始化步驟），所以自己逐層建立、逐層同步，只把競爭的那一種錯誤當成功。

**動手練習。** 結果應該和第 1、2 題的實測相同：平行的第一次失敗 3 個，第二次全過；刪掉目錄後用單一執行緒也全過。
如果你的機器上第一次就全過，不代表沒有 bug：競爭取決於執行緒的先後，可以多刪幾次目錄重跑，或直接跑第 3 節的 `repro.sh`，它在同一個上層下同時開 8 個 WAL，比較容易撞上。
