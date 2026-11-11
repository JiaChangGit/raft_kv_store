# 實際操作：從 PUT 到 Leader 恢復

這個練習要讓你看到三個獨立的 Linux process 如何選出 Leader，並在 Leader 終止後
以新的 Leader 繼續讀取。所有操作使用本機隔離資料；結果顯示的是**這次執行**，
不是對所有故障或硬體斷電的保證。

## 一次可重現的完整示範

在 WSL 的專案根目錄執行：

```bash
cd /home/user/src/Distributed_Key-Value_Store_with_Raft
. "$HOME/.cargo/env"
cargo build --workspace --locked
.venv/bin/python -m tools.learning_demo --output-dir artifacts/runs/my-first-demo
```

最後的目錄必須**尚不存在**；重跑時換一個名稱。程式使用新資料目錄，建立三個
`raft-node` process 與六條本機方向性 proxy，再透過真正的 `tools.kv_client` CLI 操作。
示範會 SIGKILL 它自己啟動的 Leader，以相同資料目錄重啟，最後關閉測試程序。
`result.json`、每個節點的 JSONL／stderr、WAL、proxy 記錄和 config 都保留在輸出目錄。

如果遇到 `Connection refused` 或逾時，先查該次目錄的 `node-*.stderr`、`node-*.jsonl`
與 `result.json`；不要把預期結果當成已經發生。測試使用臨時埠，不能照搬其他次的埠號。

另外也以 README 的固定埠與直連設定實際驗過相同流程，結果和實際 client 命令保存在
[直連實測紀錄](../artifacts/runs/manual-direct-check/result.json)。直連示範沒有使用 proxy；
上方一鍵示範則使用 proxy，兩者的網路路徑不同。

## 本次實測結果

以下是 2026-11-10 的一次實際執行，完整命令、動態埠、程序狀態與來源 hash 見
[result.json](../artifacts/runs/hands-on-20261110-02/result.json)。PID、term 與 index
每次可能不同，不能拿本表的數值當作重跑時的固定答案。
本機 `artifacts/` 不納入 Git；換機或分享時需自行保存所需證據及核對 hash。

| 操作 | 這次實際觀察 |
|---|---|
| 啟動 | 三個 PID 為 62095、62096、62097；節點 2 為 term 1 Leader |
| client 701、sequence 0：PUT `lesson-x=A` | `OK`，先前值 `null`，committed index 2 |
| sequence 1：GET | `OK`，結果 `A`，committed index 3 |
| SIGKILL Leader | 節點 2 的 PID 62096 結束，回傳碼 -9 |
| 其餘節點重新選舉 | 節點 3 成為 term 2 Leader |
| sequence 2：新 GET | `OK`，結果 `A`，committed index 5 |
| 原節點重啟 | 節點 2 使用原資料目錄，以新 PID 62364 啟動；三個節點的 debug status 均顯示 `last_applied=5`、`lesson-x=A` |

上表的 PUT 回傳 `null` 是「原本沒有值」；它不是 PUT 沒有寫入。三個節點的
debug KV 供觀察恢復使用；新的客戶端讀取仍以通過共識的 GET 回覆為準。

## 從一筆 PUT 回看程式

本次 [節點 2 trace](../artifacts/runs/hands-on-20261110-02/cluster/node-2.jsonl) 中，
第 18、19、23 行依序是 client 701／sequence 0 的 propose、WAL save 完成、index 2 的 apply。
Client 的實際成功回覆也標明 committed index 2。

第 19 行的 `durable_sequence=3` 是 WAL 交易序號，不是 log index。該行
`durable_index` 仍是 1，因為 runtime 在記錄 persist 後才呼叫 `Core::persisted`；
下一個本地 send 紀錄顯示 durable index 已是 2。可對照
[Actor::execute](../crates/raft-node/src/main.rs)、
[Core::persisted／advance_commit](../crates/raft-core/src/lib.rs) 和
[Wal::save](../crates/raft-storage/src/lib.rs)。

**實測範圍：**這些行支持同一個節點檔案中的事件順序、index 與回覆。現有 trace 的
RPC 行不包含 RPC 類型與 entry 內容，也沒有跨程序共用時間戳；不能只因幾行相鄰，
就把特定收到的訊息認定為確認這筆 PUT 的 ACK。多數確認與 barrier 的條件要另外
對照程式、proxy 記錄及測試；程式推論與本次直接觀察應分開。

## 自己逐步操作

一鍵示範適合先看完整流程；手動操作則需要三個 terminal。先確認
`config/cluster.toml` 的 7101～7103、7201～7203 沒有被其他服務使用。若有佔用，
建立自己的 config 副本並修改端點。從專案根目錄建立新目錄並記下顯示的路徑：

```bash
mkdir -p artifacts/runs
mktemp -d "$PWD/artifacts/runs/manual-XXXXXX"
```

下列 `<實驗目錄>` 請替換為剛顯示的同一個絕對路徑；三個 terminal 各自從專案根目錄執行一行：

```bash
target/debug/raft-node --id 1 --config config/cluster.toml --data-dir <實驗目錄>/node-1
target/debug/raft-node --id 2 --config config/cluster.toml --data-dir <實驗目錄>/node-2
target/debug/raft-node --id 3 --config config/cluster.toml --data-dir <實驗目錄>/node-3
```

在第四個 terminal 分別檢查 7201、7202、7203 的狀態（將下面埠號替換三次）：

```bash
.venv/bin/python -c 'import json; from tools.kv_client import call; print(json.dumps(call("127.0.0.1:7201", {"status": True}), ensure_ascii=False, indent=2))'
```

確認三個 `pid` 不同，記下 `role` 為 `Leader` 的節點、PID 與 term；狀態端點是
debug 工具。使用 README 的 client 命令送 PUT／GET，記下 `committed_index`。
這個新目錄可使用 client ID 7；若重用舊目錄，請改用新的 client ID，或正確延續 sequence。

確認目標 PID 的 `ps -p <PID> -o pid,args` 確實是**這次自己啟動的 Leader**，再使用
`kill -KILL <PID>`。觀察另外兩個節點的狀態，確認新的 Leader；以新的 sequence 再做
GET。最後使用相同 node ID 與原 `<實驗目錄>/node-N` 重啟舊節點，輪詢三個狀態的
`last_applied` 是否到達新 GET 的 index。完成後只停止這次實驗所啟動的程序。

## 試著預測

1. 如果 PUT 已 commit、回覆卻在途中遺失，client 換新 sequence 重送會怎樣？
   先讀 [client 契約](CLIENT_CONTRACT.md)；原請求應以相同 ID／sequence／內容重試，
   新 sequence 表示另一個 logical request。
2. 舊 Leader 若是被隔離而非殺掉，是否可能仍顯示 `Leader`？可能。應再看 term、
   是否有 quorum，以及新的 PUT／GET 能否合法完成；角色字串本身不是多數證據。
3. 為何本次 PUT 的 index 是 2，換 Leader 後的 GET 是 5，而不是依序 2、3、4？
   先看記錄，再追 `become_leader` 與 NoOp；本次一般 trace 不記錄 entry 內容，
   不能只憑 index 差距宣稱已直接觀察到第 4 筆的內容。

解答可與 [既有帶讀](LEARNING_GUIDE.md) 和
[Raft 論文](https://raft.github.io/raft.pdf) Figure 2、§5.4.2 對照。這份練習已由代理實跑，
是否能獨立預測與操作，仍要等你實際練習才知道。
