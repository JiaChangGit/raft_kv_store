# Raft 原始文獻筆記

用途：各階段寫規格與教學時引用的出處對照。這裡只放**短引與自己的整理**，不放論文全文。
讀取日期 2026-11-12。引用時請回到原文核對；本筆記可能有理解錯誤。

## 來源與查證方式

| 代號 | 文獻 | 取得方式 | 查證程度 |
|---|---|---|---|
| 論文 | Ongaro & Ousterhout, *In Search of an Understandable Consensus Algorithm (Extended Version)*，<https://raft.github.io/raft.pdf> | 下載 PDF，以 `pdftotext` 轉文字後閱讀 | 下列引到的段落都實際讀過。圖中表格因轉檔會打散排版，欄位以轉出的文字為準 |
| 博論 | Ongaro, *Consensus: Bridging Theory and Practice*（博士論文），<https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> | 同上 | 只讀了下列引到的段落，其餘章節未讀。章節編號取自 PDF 的目錄與內文標題 |
| 郵件 | Ongaro, "bug in single-server membership changes"，raft-dev 郵件群組，2015-07-10，<https://groups.google.com/g/raft-dev/c/t4xj6dJTP6E> | 2026-11-13 由子代理讀取網頁（本 session 沒有直接讀網頁的工具），回報逐字短引 | 引號內是子代理回報的原文短引（每段限 125 字元內）；反例的逐步狀態是**子代理的轉述**。網頁在 Jordan H. 的回覆（2015-07-10 12:34）之後被截斷，之後的回覆沒有讀到 |
| gist | Ongaro, "Safety of Raft single-server membership changes"，<https://gist.github.com/ongardie/a11f32b70581e20d6bcd> | 同上 | 只取得標題、目的與結構；論證內容未逐步核對 |

尚未讀：論文 §6 joint consensus 的完整規則（本專案不採用，只讀了開頭）；博論 §5.2 磁碟型、§5.3 漸進式；
博論 §6.4.1 lease。博論第 4 章（§4.3 以外）已於 2026-11-13 逐段讀過（見「成員變更」）。對應階段開始前補讀。博論 §5.1 已於 2026-11-13 讀過（見下）。

## 持久化狀態（S1）

論文 Figure 2，*Persistent state on all servers*，標題下註明
"Updated on stable storage before responding to RPCs"：`currentTerm`、`votedFor`、`log[]`
（"first index is 1"）。

論文 §5.2："Each server will vote for at most one candidate in a given term, on a first-come-first-served basis"；
Figure 3 的 Election Safety："at most one leader can be elected in a given term"，以及 Leader Completeness
（一筆 entry 在某任期被 commit 後，會出現在之後所有任期的 leader 的 log 中）。

對本專案的意義：這正是 WAL 要保存的三樣東西，也是「先持久化、後回覆」規則的出處。
節點若在重啟後遺失其中任一項，就可能在同一 term 投兩次票或讓已確認的 entry 消失——第一篇 bug 教學的嚴重性論證以此為據。

## 壓縮與快照（S2）

論文 §7 *Log compaction*、Figure 12、Figure 13。

- 快照的內容與中繼資料：狀態機的完整狀態，加上 "last included index"（"the index of the last entry in the log
  that the snapshot replaces (the last entry the state machine had applied)"）與 "last included term"。
  保留這兩者的理由："to support the AppendEntries consistency check for the first log entry following the snapshot"。
- 成員設定："the snapshot also includes the latest configuration in the log as of last included index"。
- 誰來做：每台節點各自做快照，只涵蓋已 commit 的 entry；完成後可刪除到 last included index 為止的 log 與較舊的快照。
- 何時由 leader 傳送：當 leader "has already discarded the next log entry that it needs to send to a follower"，
  典型情況是非常慢的 follower 或新加入的節點。
- **InstallSnapshot RPC（Figure 13）**
  - 參數：`term`、`leaderId`、`lastIncludedIndex`、`lastIncludedTerm`、`offset`、`data[]`、`done`。
  - 回覆：`term`（"currentTerm, for leader to update itself"）。
  - 接收端步驟（原文 8 步）：1. `term < currentTerm` 立即回覆；2. `offset` 為 0 時建立新快照檔；3. 在指定 offset 寫入資料；
    4. `done` 為 false 則回覆並等待後續；5. 儲存快照檔，丟棄 index 較小的既有或未完成快照；
    6. "If existing log entry has same index and term as snapshot's last included entry, retain log entries following it and reply"；
    7. 丟棄整份 log；8. 以快照內容重設狀態機，並載入快照中的成員設定。
  - 分塊傳送的理由：每一塊都讓 follower 知道 leader 還活著，可重設選舉計時器。
- follower 收到快照時：快照比自己的 log 新 → 丟棄整份 log（其中可能有與快照衝突的未 commit entry）；
  快照只是自己 log 的前綴 → 刪除被涵蓋的部分，**保留後面的 entry**。
- 何時做快照：論文建議的簡單策略是 log 達到固定大小時。寫快照不應阻塞正常運作，論文提到 copy-on-write。

去重 session 必須放進快照：博論 §5.1 明文寫道 "State machines must also serialize the information they keep for
providing linearizability to clients (see Chapter 6)"。（2026-11-12 的筆記曾把這點標為推論，2026-11-13 讀到原文後更正。）

博論 §5.1 *Snapshotting memory-based state machines* 的其他要點：

- 做完快照後 "Raft first stores the state it needs for a restart: the index and term of the last entry included in the snapshot
  and the latest configuration as of that index. Then it discards the prefix of its log up through that index."
- §5.1.1 與正常處理同時做快照：copy-on-write 或 fork；記憶體不足時寧可暫停接受新 entry，也不要中止快照再重試。
- §5.1.2 何時做快照：簡單做法是 log 達固定大小；較好的做法是 log 超過「上次快照大小 × 可設定的倍數」，
  "an expansion factor of 4 results in about 20% of the disk's bandwidth being used towards snapshotting"。
- §5.1.3 實作注意事項：快照先寫暫存檔、flush 後 rename，避免啟動時讀到寫一半的快照；建議對快照加 checksum；
  程式原本假設 "if entry i was present in the log, entries 1 through i − 1 would also be present"，壓縮後不成立，
  "This would have been easier with help from a more powerful type system"；
  "We recommend taking snapshots after applying every log entry during development, since that can help catch bugs quickly"；
  在 log 真的能丟棄之前，很多新路徑無法被系統測試執行到，實作順序要事先規劃。

## 唯讀請求（S3）

論文 §8 *Client interaction* 末段；博論 §6.4 *Processing read-only queries more efficiently*。

論文指出不寫 log 的讀取需要兩項額外措施：leader 必須知道哪些 entry 已 commit，
所以 "each leader commit a blank no-op entry into the log at the start of its term"；
以及回覆前 "exchange heartbeat messages with a majority of the cluster"，以確認自己沒有被取代。

博論 §6.4 的五個步驟：

1. leader 若尚未把本任期的任何 entry 標為 committed，先等到完成（靠任期開始時的 no-op）。
2. 把目前的 commit index 記為 `readIndex`。
3. 送出新一輪 heartbeat，等待多數回覆。
4. 等狀態機 apply 到至少 `readIndex`。
5. 執行查詢並回覆。

補充：一輪 heartbeat 可以同時服務累積的多筆讀取。follower 也能服務讀取，但必須先向 leader 取得 `readIndex`
再自行做第 4、5 步；否則有讀到舊資料的風險。lease 做法依賴時鐘誤差有上界（論文稱 "bounded clock skew"），本專案不採用。

## 用戶端重試與 session（既有功能，S2 會動到）

論文 §8：用戶端為每個命令指定唯一序號，狀態機記錄每個用戶端最後處理的序號與對應回覆，重複的命令直接回覆、不再執行。

博論 §6.3：session 不能永久保存，但 "session expiry must be deterministic, just as normal state machine operations must be"，
否則各節點的狀態機會分歧。兩種做法：限制 session 數量並以 LRU 淘汰；或以大家同意的時間來源判斷（LogCabin 由 leader 在
每筆命令附上時間）。本專案目前 session 無上限，要不要加淘汰在 S2 決定；若加，必須是由 log 內容決定的確定性規則。

## 成員變更（S4）

本專案採博論第 4 章的單步變更，不採論文 §6 的 joint consensus。2026-11-13 寫 S4 規格（`docs/MEMBERSHIP.md`）前，
博論 §4.1、§4.2.1～§4.2.4、§4.4、§4.5 逐段讀過（以 `pdftotext` 轉出的文字；頁碼是 PDF 內印的頁碼）；§4.3 只讀了開頭。

博論 §4.1 *Safety*（頁 35～37）：

- 一次只增減一台時，舊叢集的任一多數與新叢集的任一多數必然重疊（"the voter" 必然存在），所以可以直接切換。
- 設定以特殊的 log entry 表示。**"The new configuration takes effect on each server as soon as it is added to that
  server's log"**；節點 "do not wait for configuration entries to be committed, and each server always uses the latest
  configuration found in its log"。
- Cnew 被 commit 才算完成，之後才能開始下一次變更。理由（頁 36）：若節點等到得知 commit 才採用新設定，leader 很難知道舊叢集的多數何時已採用，
  而且 "the servers would need to persist their commit index to disk; neither of these mechanisms is required in Raft"。
- 設定 entry 可能因 leader 更替而被移除，此時節點 "must be prepared to fall back to the previous configuration in its log"。
- 處理 RPC 時以**呼叫者**的設定為準：節點會接受不在自己最新設定內的 leader 的 AppendEntries，也會投票給不在自己最新設定內的候選人。

博論 §4.2 *Availability*：

- §4.2.1 追趕新節點（頁 38～39）：新節點先以不投票的身分接收 log。"The replication of entries to the new server is split into rounds"，
  每一輪複製該輪開始時 leader log 中的全部 entry。**"The algorithm waits a fixed number of rounds (such as 10). If the last round
  lasts less than an election timeout, then the leader adds the new server to the cluster"**，否則中止並回報錯誤，呼叫者可以重試。
  "The leader should also abort the change if the new server is unavailable or is so slow that it will never catch up"；
  論文舉了 Lamport 的 Paxos 議會 "changed the membership to consist of only drowned sailors"，以及作者第一次變更就 "included a typo in a network port number"。
  另建議 "have followers return the length of their logs in the AppendEntries response; this allows the leader to cap the follower's nextIndex accordingly"。
- §4.2.2 移除現任 leader（頁 39～40）："a leader that is removed from the configuration steps down once the Cnew entry is committed"。
  太早退位可能讓它又逾時當選而拖慢進度；從兩台叢集移除 leader 時，它甚至必須再當選（Figure 4.6）。Cnew commit 後
  "it will always be possible for the members of Cnew to elect a new leader from among themselves"。
  在那之前 leader "replicates log entries but does not count itself in majorities"；不在自己最新設定中的節點仍要發起選舉，
  "It does not count its own vote in elections unless it is part of its latest configuration"。
- §4.2.3 干擾的節點（頁 40～42）：被移除的節點收不到 heartbeat，"it will not receive the Cnew entry or learn of that entry's commitment"，
  會以新 term 不斷發起選舉。PreVote 式的檢查不夠（Figure 4.7）。解法："if a server receives a RequestVote request within
  the minimum election timeout of hearing from a current leader, it does not update its term or grant its vote"；
  "It can either drop the request, reply with a vote denial, or delay the request; the result is essentially the same"。
  正常選舉不受影響，因為每台至少等最小選舉逾時才參選。這與 leadership transfer 衝突（本專案不做 leadership transfer）。
- §4.2.4 可用性論證（頁 42～43）：假設 "a majority of the old configuration is available (at least until Cnew is committed) and that a majority
  of the new configuration is available"。leader 能把 heartbeat 送到自己的設定時，它與 follower
  "will ignore any RequestVote messages with a higher term from other servers"。退位的 leader 有很小的風險 "might become leader again"，
  再當選後 "would confirm the commitment of the Cnew entry and soon step down"。

博論 §4.4 *System integration*（頁 45～46）：自動增減節點要有明確政策；替換節點時 "it is preferable to add servers before removing servers"；
引導叢集時 "one server is initialized with a configuration entry as the first entry in its log. This configuration lists only that one
server; it alone forms a majority of its configuration"，其他節點以空 log 啟動、經由成員變更加入；靜態設定檔 "potentially error-prone"。

博論 §4.5（頁 46）："fault-tolerance requirements can change over time, and failed servers eventually need to be replaced"。

**2015 年的問題（郵件，2026-11-13 經子代理讀取）**：

- 原規則（Diego Ongaro）："A leader may only create a configuration entry in its log if the prior one is committed"，
  且 "the new one differs from the prior one by at most one server"；"Each server always uses the latest configuration in its log,
  regardless of whether that entry is committed, for counting votes and determining commitment."
- 三個反例都從四台 C = {S1..S4} 開始（Huanchen Zhang 的工作，Diego 整理）。以下是**子代理的轉述**：
  1. 一加一減：S1（term 1 leader）附加 D = {S1..S5}，只送到 S5 就離線；S2 以 {S2,S3,S4} 的票贏得 term 2，附加 E = {S2,S3,S4}，送到 S3 即 commit；
     S1 以 {S1,S4,S5} 的票贏得 term 3，把 D 複製給所有人，覆蓋已 commit 的 E。
  2. 兩次加入：前三步相同，S2 改為追趕 S6 並附加 E = {S1,S2,S3,S4,S6}，送到 S3、S6 即 commit；S1 以 {S1,S4,S5} 贏得 term 3 並覆蓋 E。
  3. 兩次移除：S1 附加 D = {S1,S2,S3} 後離線；S2 以 {S2,S3,S4} 贏得 term 2，附加 E = {S1,S2,S4}，送到 S4 即 commit；S1 以 {S1,S3} 贏得 term 3 並覆蓋 E。
- 發生條件（原文）："Starting from an even-sized cluster," "Multiple different changes are requested concurrently," "Leadership is lost."；
  後果 "data loss and permanent split brain could occur"。
- 修正（原文）："a leader may not append a new configuration entry until it has committed an entry from its current term"；
  "This change would mean rejecting or delaying membership change requests until the no-op entry is committed"。
- 理由（原文）："Once a leader has committed an entry in its current term, it knows it has the latest committed configuration,
  and no existing uncommitted configurations from prior terms can be committed anymore."
  Diego 轉述 Ousterhout："if two configurations "compete", they differ by at most one server and hence have overlapping consensuses"。
- "We don't yet have a formal safety proof for the correctness of membership changes (of either type) in Raft."；
  joint consensus 不受影響。gist 是非正式論證，延伸論文的安全性論證中 "the voter" 的那一步。

**為什麼需要偶數台（我的推導，不是原文）**：設原設定 C 有 n 台，兩個競爭的設定 D、E 各與 C 差一台。
要讓 D 與 E 的多數不重疊，需要 `多數(D) + 多數(E) ≤ |D ∪ E|`。
一加一減：`|D| = n+1`、`|E| = n-1`、`|D ∪ E| = n+1`；n = 4 時 3 + 2 = 5 ≤ 5，可能不重疊；n = 3 時 3 + 2 = 5 > 4，必然重疊。
兩次加入：`|D ∪ E| = n+2`，n = 4 時 3 + 3 = 6 ≤ 6；n = 3 時 3 + 3 = 6 > 5。
兩次移除：`|D ∪ E| = n`，n = 4 時 2 + 2 = 4 ≤ 4；n = 3 時 2 + 2 = 4 > 3。
n 為奇數時三種組合都必然重疊，與郵件的 "Starting from an even-sized cluster" 一致。S4 模擬器只有 5 個槽位，所以能出現的是 n = 2 與 n = 4 的組合
（n = 4 的「兩次加入」需要 6 台，模擬器做不到）。

## 各功能之間的交會點

以下是我讀完後的整理，屬於推論，各階段規格需逐條確認：

- 任期開始的 no-op 被 commit，同時是 ReadIndex（博論 §6.4 第 1 步）與成員變更（2015 修正）的前提。本專案 V1 已有 no-op。
- 「多數」在 S4 之後取決於 log 中最新的設定，而快照截掉了前段 log，所以快照必須帶著設定（論文 §7 已要求）。
- 快照涵蓋的範圍只到已 apply 的位置；設定 entry 則是寫入即生效、可能尚未 commit。快照中的設定因此一定是已 commit 的那一份。
- ReadIndex 的 heartbeat 多數與成員變更進行中的多數必須用同一份設定計算。
