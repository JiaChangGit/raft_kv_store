# Client 與 deterministic KV 契約

client_id 為穩定 u128，sequence 為嚴格遞增 u64。每個 client 一次僅有一筆
logical outstanding request；timeout／斷線後保留同一 ID、sequence、operation 重試。
初始 sequence 可為任意值，新 logical request 必須增加。新 GET 也需要新 sequence。

Command 是 NoOp、Client(Put/Delete/Get) 或 Config（成員設定，S4）。NoOp 與 Config 只能由 Raft 產生；
狀態機套用 Config 時只前進 applied index，不改變 KV（與 NoOp 相同）。
key 上限 256 UTF-8 bytes，value 上限 4096 bytes；runtime 在接受入 log 前檢查。
state machine 也防禦性拒絕非法尺寸與未曾見過卻低於該 client 最高 sequence 的請求。

StateMachine::apply 只接受已確認 committed 的連續 prefix，index 必須恰好等於
applied_index+1。它不判定 commit，也不讀 WAL。NoOp 僅推進 applied index。
Put 回傳先前 value（原本不存在為 null），Delete 回傳移除的 value（不存在為 null），
Get 回傳當時 value（不存在為 null）。所有值皆為字串；null 與空字串不同。

session 以 client_id 為鍵，每個 client 只保存**最新一筆**寫入的 sequence、完整 typed operation
與原 ClientResponse（S2 起，見 [快照規格](SNAPSHOT_COMPACTION.md) §6.1）。
相同 ID 與內容的重送回傳原結果與原 committed_index，且不重新執行操作；同 ID
不同內容回 REQUEST_ID_REUSE，不能改 KV。sequence 小於該 client 最新一筆的請求回
INVALID_ARGUMENT，同樣不執行：正確的 client 一次只有一筆未完成請求，不會重送更舊的；
同一請求的延遲副本若在更新的請求之後才被 commit，也只會得到這個拒絕，不會重複執行。不同 leader 可將同一 request 寫進多筆
log；這不影響同一 logical request 只產生一次效果。

GET（S3 起，見 [ReadIndex 規格](READ_INDEX.md)）不寫進 Raft log，也不進入 session：只有已 commit 本任期 entry、
並在讀取到達後以多數確認自己仍是 leader 的節點才能回覆，且狀態機至少已套用到 readIndex。GET 重送時重新讀取，
結果可能比第一次新，這仍是線性一致的。GET 不做 sequence 檢查，也不和寫入共用去重紀錄；
用戶端仍應為每個新請求（包括 GET）遞增 sequence。follower 收到 GET 回 NOT_LEADER；leader 失去身分時等待中的 GET
也回 NOT_LEADER，因為讀取沒有副作用。回覆的 committed_index 是讀取時狀態機已套用到的 index。

timeout 表示 TIMEOUT_UNKNOWN：操作可能未執行，也可能已提交但回覆遺失。
NOT_LEADER 的 hint 不具永久權威。REQUEST_ID_REUSE 與 INVALID_ARGUMENT 是明確
拒絕；STORAGE_ERROR 不能轉成 RAM-only 成功。
STORAGE_ERROR 也不能推定操作一定沒有發生：完整但未成功同步的 record 可能在重啟後
存活，之後依合法共識提交。若需重試，仍保持相同 ID／sequence／內容；獨立 history
checker 對這個狀態採不確定結果分支，而不直接刪除該操作。

recovery 先從已落盤的快照還原 KV/session（沒有快照則為空），再重放重新確認 committed 的後續
entry，重建相同值與原結果。session 數等於 client 數，沒有依時間淘汰；client 數本身沒有上限。
effect_count 是首次執行的請求數，重複的 log entry 不會增加它。S3 起 GET 不進 log，所以新的 GET 不會計入；
log 中既有的 Get entry（例如 S3 之前寫下的資料）仍照 S2 的規則套用，也會計入一次。

成員變更請求（S4，見[成員變更規格](MEMBERSHIP.md) §10）走同一個 client 連接埠：
`{"version":1,"membership":{"Add":4}}` 或 `{"version":1,"membership":{"Remove":2}}`。回覆 OK 時 `result` 是新設定（例如 `"1,2,3,4"`），
`committed_index` 是該設定 entry 的 index。其他狀態：NOT_LEADER（附 hint）、CHANGE_IN_PROGRESS（稍後重試）、CATCH_UP_FAILED（新節點追不上，
設定不變）、INVALID_ARGUMENT（編號不合法、要加入的節點不在位址表中，或會移除最後一台）、TIMEOUT_UNKNOWN（節點程式的 15 秒等待逾時；設定 entry
可能已附加，之後仍可能 commit）。成員變更請求沒有 client_id／sequence，不進 session；重送同一個變更，若已生效會立即回 OK。

測試結果由階段證據另行記錄，不將本契約文字當作已執行驗證。
