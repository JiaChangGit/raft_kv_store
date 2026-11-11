# Raft 原始文獻筆記

用途：各階段寫規格與教學時引用的出處對照。這裡只放**短引與自己的整理**，不放論文全文。
讀取日期 2026-11-12。引用時請回到原文核對；本筆記可能有理解錯誤。

## 來源與查證方式

| 代號 | 文獻 | 取得方式 | 查證程度 |
|---|---|---|---|
| 論文 | Ongaro & Ousterhout, *In Search of an Understandable Consensus Algorithm (Extended Version)*，<https://raft.github.io/raft.pdf> | 下載 PDF，以 `pdftotext` 轉文字後閱讀 | 下列引到的段落都實際讀過。圖中表格因轉檔會打散排版，欄位以轉出的文字為準 |
| 博論 | Ongaro, *Consensus: Bridging Theory and Practice*（博士論文），<https://web.stanford.edu/~ouster/cgi-bin/papers/OngaroPhD.pdf> | 同上 | 只讀了下列引到的段落，其餘章節未讀。章節編號取自 PDF 的目錄與內文標題 |
| 郵件 | Ongaro, "bug in single-server membership changes"，raft-dev 郵件群組，2015-07-10，<https://groups.google.com/g/raft-dev/c/t4xj6dJTP6E> | 由子代理讀取網頁後摘要 | **我沒有親自讀原頁**；S4 開始前必須重讀原文確認 |

尚未讀：論文 §6 joint consensus 的完整規則（本專案不採用，只讀了開頭）；博論第 5 章各節
（5.1 記憶體型快照、5.2 磁碟型、5.3 漸進式、InstallSnapshot 圖）；博論 §4.2.1 的完整演算法；
博論 §6.4.1 lease。對應階段開始前補讀。

## 持久化狀態（S1）

論文 Figure 2，*Persistent state on all servers*，標題下註明
"Updated on stable storage before responding to RPCs"：`currentTerm`、`votedFor`、`log[]`
（"first index is 1"）。

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

去重 session 是否必須放進快照：論文與已讀的博論段落**沒有明文**。這是本專案的推論：session 屬於狀態機狀態，
而快照必須足以重建狀態機，所以要放。S2 規格需把它列為設計決定並以測試驗證（快照後重試不會重複執行）。

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

本專案採博論第 4 章的單步變更，不採論文 §6 的 joint consensus。

博論 §4.1 *Safety*：

- 一次只增減一台時，舊叢集的任一多數與新叢集的任一多數必然重疊，所以可以直接切換。
- 設定以特殊的 log entry 表示。**"The new configuration takes effect on each server as soon as it is added to that
  server's log"**；節點 "do not wait for configuration entries to be committed, and each server always uses the latest
  configuration found in its log"。
- Cnew 這筆 entry 被 commit 才算變更完成；之後才能開始下一次變更。
- 設定 entry 可能因 leader 更替而被移除，此時節點 "must be prepared to fall back to the previous configuration in its log"。
- 處理 RPC 時以**呼叫者**的設定為準：節點會接受不在自己最新設定內的 leader 的 AppendEntries，也會投票給不在自己最新設定內的候選人。
  理由是否則新節點永遠無法加入，且某些情況需要這張票才能維持可用。

博論 §4.2 *Availability*：

- §4.2.1 追趕新節點：新節點先以不投票的身分接收 log，分回合複製，最後一回合夠短才正式加入（細節未完整閱讀）。
- §4.2.2 移除現任 leader："a leader that is removed from the configuration steps down once the Cnew entry is committed"。
  太早退位可能讓它又逾時當選而拖慢進度。
- §4.2.3 干擾的節點：被移除的節點收不到 heartbeat 會不斷發起選舉。解法："if a server receives a RequestVote request within
  the minimum election timeout of hearing from a current leader, it does not update its term or grant its vote"。
  這與 leadership transfer 衝突（本專案不做 leadership transfer）。

**2015 年發現的問題（郵件，未親讀原文）**：單步變更在跨任期時可能出現不重疊的多數。
修正是 leader 必須先 commit 一筆自己任期的 entry，才能附加新的設定 entry；實務上就是等任期開始的 no-op 被 commit。
摘要另提到作者當時表示尚無正式的安全性證明。**S4 規格必須包含這條規則與對應的反例測試，並在開始前重讀原文。**

## 各功能之間的交會點

以下是我讀完後的整理，屬於推論，各階段規格需逐條確認：

- 任期開始的 no-op 被 commit，同時是 ReadIndex（博論 §6.4 第 1 步）與成員變更（2015 修正）的前提。本專案 V1 已有 no-op。
- 「多數」在 S4 之後取決於 log 中最新的設定，而快照截掉了前段 log，所以快照必須帶著設定（論文 §7 已要求）。
- 快照涵蓋的範圍只到已 apply 的位置；設定 entry 則是寫入即生效、可能尚未 commit。快照中的設定因此一定是已 commit 的那一份。
- ReadIndex 的 heartbeat 多數與成員變更進行中的多數必須用同一份設定計算。
