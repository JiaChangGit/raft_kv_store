# 實際操作：寫入、終止 leader、再恢復

這個練習讓你親眼看到三件事：三個獨立的 process 組成叢集並選出 leader；寫入的資料在 leader 被終止後仍然讀得到；
被終止的節點用原本的資料目錄重新啟動後，會追上其他節點。

先照 [從零開始](SETUP.md) 建好專案。下面的結果是 2026-11-17 的一次實際執行；PID 每次都不同，term 與 index 通常相同，但不保證。

## 一行命令跑完整個示範

```bash
.venv/bin/python -m tools.learning_demo --output-dir artifacts/runs/my-first-demo
```

最後的目錄必須還不存在，重跑時換一個名稱。這支腳本做的事：

1. 在新的目錄啟動三個 `raft-node` process，節點之間的訊息都經過本機的 proxy（和故障測試用的是同一套）。節點 1 引導，再加入節點 2、3。
2. 用真正的用戶端命令 `tools.kv_client` 寫入 `lesson-x=A`，再讀出來。
3. 在 leader 的紀錄裡找出這筆寫入的三個時間點：提出、落盤、套用，確認順序正確。
4. 用 SIGKILL 終止 leader，等另外兩台選出新的 leader，再讀一次。
5. 用原本的資料目錄重新啟動被終止的節點，等三台都套用到同一個位置。

結束時印出 `結果：.../result.json`，`"status": "PASS"` 就是每一步都符合預期。輸出目錄裡還有每個節點的紀錄（`cluster/node-*.jsonl`）、WAL 與設定檔。
失敗時 `result.json` 的 `error` 寫著停在哪一步；再看對應節點的 `node-*.stderr` 與 `node-*.jsonl`。

## 這次看到的結果

| 步驟 | 看到的 |
|---|---|
| 啟動 | 三個 PID 是 469613、469614、469615；節點 1 是 term 2 的 leader，成員 `[1, 2, 3]`，commit index 4 |
| PUT `lesson-x=A`（client 701、sequence 0） | `OK`，舊值 `null`，committed index 5 |
| GET（sequence 1） | `OK`，結果 `A`，committed index 5 |
| SIGKILL leader | 節點 1（PID 469613）結束，回傳碼 -9 |
| 重新選舉 | 節點 2 成為 term 3 的 leader |
| 新的 GET（sequence 2） | `OK`，結果 `A`，committed index 6 |
| 節點 1 重新啟動 | 新的 PID 469794；三台的 `last_applied` 都是 6，`lesson-x` 都是 `A` |

### 這些數字是怎麼來的

**為什麼 leader 是 term 2，不是 term 1？** 引導時，節點 1 的資料目錄寫進了 term 1 與一筆「成員只有自己」的設定（index 1）。
它啟動後要經過一次選舉才成為 leader，選舉把 term 加一，所以是 term 2。

**為什麼一開始 commit index 就是 4？** 寫入之前，log 裡已經有四筆 entry：

| index | 內容 | 誰寫的 |
|---|---|---|
| 1 | 成員設定 `[1]` | 引導 |
| 2 | NoOp | 節點 1 當選後立刻寫入（新 leader 都會先寫一筆，見 [自問自答](FAQ.md) Q7） |
| 3 | 成員設定 `[1, 2]` | `add 2` |
| 4 | 成員設定 `[1, 2, 3]` | `add 3` |

所以第一筆 PUT 是 index 5。

**為什麼 GET 的 committed index 和 PUT 一樣是 5？** GET 不寫進 log（ReadIndex），回覆裡的 committed index 是 leader 回答時狀態機已經套用到的位置。
讀取沒有讓 log 變長。

**為什麼換 leader 之後變成 6？** 節點 2 當選 term 3 的 leader 後，也先寫一筆 NoOp，那是 index 6。等它 commit 之後才能回答讀取（[ReadIndex 規格](READ_INDEX.md) §6 的第一步），
所以新的 GET 回 6，而值仍然是 `A`：已經 commit 的寫入不會因為換 leader 而消失。

**重新啟動的節點怎麼追上？** 它從磁碟讀回 term、投票與 log（到 index 5），但不知道哪些已經 commit，所以先當 follower。
新的 leader 送來 AppendEntries，它補上 index 6，得知 commit index，再依序套用。

## 從紀錄看一筆寫入

示範在 leader 的紀錄（這次是 `cluster/node-1.jsonl`）裡找到這筆 PUT 的三行：

| 行 | 事件 | `durable_index` | `last_log_index` | `commit_index` |
|---|---|---|---|---|
| 37 | `propose_batch`，`writes: 1` | 4 | 4 | 4 |
| 38 | `persist`（`fsync_us` 約 1760） | 4 | 5 | 4 |
| 42 | `apply`，`index: 5` | 5 | 5 | 5 |

- 第 37 行：actor 把這一輪收到的寫入（只有一筆）交給 core。
- 第 38 行：WAL 寫入並同步完成，一次 `fsync` 約 1.8 毫秒。`last_log_index` 已經是 5，`durable_index` 卻還是 4：
  這行紀錄寫在 actor 通知 core「已落盤」之前，通知之後 core 才把 `durable_index` 改成 5。
- 第 39、40 行：送給節點 2、3 的訊息（`rpc_send`）。第 41 行：節點 3 的回覆到了（`message`，`rpc_id` 12 對應第 40 行）。
- 第 42 行：節點 1 自己加上節點 3 已經是多數，index 5 commit 並套用到狀態機。不必等節點 2 的回覆。

這些行只能證明**同一個節點內**事件的先後。不同節點的紀錄沒有共用的時鐘，不能只靠時間戳說哪個回覆確認了哪一筆寫入；
那要對照程式（[架構說明](ARCHITECTURE.md)「一次寫入」）與測試。

## 自己一步一步做

一鍵示範看完整流程；想自己動手，照 [從零開始](SETUP.md) 第 5 節啟動三台節點、加入成員、寫入與讀取，再接著做：

1. 查每台的狀態，找出 leader：

   ```bash
   .venv/bin/python tools/kv_admin.py --config config/cluster.toml status
   ```

   記下 leader 的編號與 term。只想看某一台的完整狀態時：

   ```bash
   .venv/bin/python -c 'import json; from tools.kv_client import call; print(json.dumps(call("127.0.0.1:7201", {"status": True}), ensure_ascii=False, indent=2))'
   ```

   把 7201 換成 7202、7203 就是另外兩台。這是除錯用的狀態，不是正式的讀取。

2. 在 leader 的 terminal 按 Ctrl-C 停掉它（或用 `kill -KILL <PID>` 模擬當機；先用 `ps -p <PID> -o pid,args` 確認那是你自己啟動的節點）。
3. 等一兩秒，再查一次狀態：另外兩台之一成為新的 leader，term 加一。
4. 用新的 sequence 讀 x，值仍然是 `A`。
5. 用原本那一行命令重新啟動舊的 leader，再查狀態，看它的 `last_applied` 追上其他兩台。

用 client ID 7 時，sequence 要接著用下去（2、3、4……）。換一個新的資料目錄重來時，也可以從 0 開始。

## 試著預測

先寫下你的答案，再做一次看看。

1. PUT 已經 commit，回覆卻在路上遺失了。client 用**新的** sequence 重送同一筆 PUT，會發生什麼？用**同一個** sequence 呢？
2. 舊的 leader 不是被終止，而是網路斷了。它的狀態會不會仍然顯示 `Leader`？它還能回答讀取嗎？
3. 三台中停掉兩台，剩下的那一台能寫入嗎？能讀取嗎？

解答：

1. 新的 sequence 是另一個請求，會再執行一次：值仍是 `A`，但回覆的舊值是 `A`，不是 `null`。同一個 sequence 會拿到第一次的結果，不會再執行（[client 契約](CLIENT_CONTRACT.md)）。
2. 可能仍然顯示 `Leader`，因為它還沒收到更大的 term。但它回答讀取前要先得到多數確認，被隔離時拿不到，讀取會逾時，不會讀到舊值（[ReadIndex 規格](READ_INDEX.md)；測試 `test_a_leader_left_with_a_minority_does_not_answer_reads`）。
3. 都不能。成員設定還是三台，多數是兩台，剩下一台湊不到多數：寫入無法 commit，讀取也無法確認。要等另一台回來。
