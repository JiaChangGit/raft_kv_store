# V1 實作報告

> **歷史文件：V1（2026-11-12 以前）的實作報告。** 之後的變更：S1 改用 [WAL 格式 2](STORAGE_FORMAT_V2.md)、S2 加入[快照與 log 壓縮](SNAPSHOT_COMPACTION.md)、S3 的 GET 改走 [ReadIndex](READ_INDEX.md)（不再寫進 log）、S4 加入[成員變更](MEMBERSHIP.md)（節點 1～7，多數依目前的設定計算）、S5 做了[效能改善](PERFORMANCE.md)。現在的結構見[架構說明](ARCHITECTURE.md)。

## 成果與來源

固定三個Linux process、IDs1/2/3、quorum2、一個Raftgroup。七個Rustcrates的責任見
[README](../README.md)。原始三份Markdown未改，來源hash見SOURCE_MANIFEST；
缺少的原始ZIP/gate未被冒名重建。驗收契約是acceptance/v1.json。

## 共識與資料路徑

`Core::step`只接收typed events並產生有序effects。core不存取OSclock/socket/fs；
runtime與sim使用同一Core。單actor擁有Core/Wal/StateMachine，其他threads經有界channel
交付事件。Persist在批次最前，Wal.save成功才呼叫persisted更新local durable match，
然後完成dependent sends/apply，整批結束才讀下一個事件。storage錯誤停止服務。

Figure2/§5.2選舉對應begin_election/message：durable term/selfvote、logfreshness、
唯一voter與rpc correlation。Figure2/§5.3複製對應replicate/message：每peer一個有效
AE correlation，timeout後舊回覆無效；Follower只確認該request的連續prefix。

§5.4.2直接commit條件為N>commit、log[N].term==currentTerm、兩個durablematches。
安全推導：投票freshness與多數交集保留已提交prefix；current-term entry提交後較早prefix
間接提交。這是依論文的工程推導，不是對這份Rust機器碼的形式化證明。
I1～I5 observer獨立保存歷史，並在當選transition及事件完成時核對。

GET和PUT/DELETE一樣排進log；成功在合法commit和本地KVapply後。Session完整保留
canonicalcommand/原result/原index，duplicate無第二次效果；unknown使用同ID重試。
Clientwire version1在入log前驗證，log只保存canonicalcommand，未知欄位拒絕。

## 持久化與回復

CRC32C binaryheader＋deterministic JSONtransaction，包含hardstate與atomictruncate+append。
identity與新檔案/父目錄有sync，單writer檔案鎖；最大payload有界。
只截斷可辨識incomplete tail，完整CRC錯誤failclosed。參見STORAGE_FORMAT_V1。

Restart還原term/vote/log，Follower、commit/apply0、KV/sessionempty；
透過合法leaderCommit或新Leadercurrent-termNoOp重新確認prefix，依序重建KV/session。
WAL存在不是commit證據。Crashfailpoints涵蓋write/sync/ACK/commit/apply/reply邊界；
真實subprocess和node測試範圍及限制見VERIFICATION_FINDINGS。

## 驗證與效能

實跑結果、首敗與修正見[驗證紀錄](VERIFICATION_FINDINGS.md)，最新逐階段結果讀
`artifacts/acceptance-v1/`。報告數字與rawsamples在`artifacts/reports/`及`artifacts/runs/`。
benchmark包括單client、8clients、隔離follower、leadercrash/recovery與catchup。
sync_us量的是整個Wal.save(encoding/write/sync)，不冒稱純fsync syscall。
另由Wal::save在File::sync_all呼叫前後直接計時，單獨輸出fsync_us並報告wal_fsync_us百分位，
兩組樣本數一致；這是OS呼叫返回延遲，不是獨立裝置層持久化量測。
不同run的百分位、CPU/RSS與吞吐不能混合；小樣本p99僅描述該次有限樣本。

## 明確取捨

同步簡單barrier優先於背景I/Opipeline；JSON便於debug；沒有snapshot/sessionGC所以資源
持續增長。複製corestate與線性掃描是有限規模V1的實作取捨，不作高效能productionclaim。
無membership/TLS/sharding/multiRaft/readlease/readindex。
Framework配置、實際讀取skills與效果觀察見FRAMEWORK_FEEDBACK，不能等同自動觸發率。
