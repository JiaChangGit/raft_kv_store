# Client 與 deterministic KV 契約

client_id 為穩定 u128，sequence 為嚴格遞增 u64。每個 client 一次僅有一筆
logical outstanding request；timeout／斷線後保留同一 ID、sequence、operation 重試。
初始 sequence 可為任意值，新 logical request 必須增加。新 GET 也需要新 sequence。

Command 是 NoOp 或 Client(Put/Delete/Get)。NoOp 只能由 Raft 產生。
key 上限 256 UTF-8 bytes，value 上限 4096 bytes；runtime 在接受入 log 前檢查。
state machine 也防禦性拒絕非法尺寸與未曾見過卻低於該 client 最高 sequence 的請求。

StateMachine::apply 只接受已確認 committed 的連續 prefix，index 必須恰好等於
applied_index+1。它不判定 commit，也不讀 WAL。NoOp 僅推進 applied index。
Put 回傳先前 value（原本不存在為 null），Delete 回傳移除的 value（不存在為 null），
Get 回傳當時 value（不存在為 null）。所有值皆為字串；null 與空字串不同。

session key 為 (client_id,sequence)，保存完整 typed operation 與原 ClientResponse。
相同 ID 與內容的重送回傳原結果與原 committed_index，且不重新執行操作；同 ID
不同內容回 REQUEST_ID_REUSE，不能改 KV。不同 leader 可將同一 request 寫進多筆
log；這不影響同一 logical request 只產生一次效果。

GET 重送的舊結果是原 logical read，不是新讀取。runtime 仍須經安全的
committed/apply path 才能回覆 duplicate，不能讓隔離舊 leader 用 session cache
直接回成功。新的 GET 必須進 Raft log；append 本身不等於成功，須 commit+local apply。

timeout 表示 TIMEOUT_UNKNOWN：操作可能未執行，也可能已提交但回覆遺失。
NOT_LEADER 的 hint 不具永久權威。REQUEST_ID_REUSE 與 INVALID_ARGUMENT 是明確
拒絕；STORAGE_ERROR 不能轉成 RAM-only 成功。
STORAGE_ERROR 也不能推定操作一定沒有發生：完整但未成功同步的 record 可能在重啟後
存活，之後依合法共識提交。若需重試，仍保持相同 ID／sequence／內容；獨立 history
checker 對這個狀態採不確定結果分支，而不直接刪除該操作。

recovery 從空 KV/session 重放重新確認 committed 的 prefix，重建相同值與原結果。
V1 不做 session GC；記憶體與 WAL 都可能持續成長，故障實驗與 benchmark 應限制規模。
effect_count 是首次執行的 logical request 數，含 GET；不等於 log entry 數或寫入數。

測試結果由階段證據另行記錄，不將本契約文字當作已執行驗證。
