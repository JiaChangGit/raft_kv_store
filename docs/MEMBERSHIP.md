# S4 規格：成員變更（一次一台）

狀態：**已核准（2026-11-13，使用者：「依你建議，繼續完成」），實作中；本文件的保證都還沒有被測試驗證。**
已確認的決定見 §17。實作完成後在 §18 補記實際做法與證據。

用語：
- **設定**（configuration）：哪些節點有投票權。本文寫作 `{1,2,3}`。**Cold／Cnew**：一次變更前後的設定。
- **投票成員**（voter）：在目前設定中的節點；選舉與 commit 只計算投票成員。
- **追趕**（catch-up）：新節點在正式加入前，先以不投票的身分接收 log。
- **引導**（bootstrap）：叢集第一次建立時，讓第一台節點擁有一份「只含自己」的設定。
- **最小選舉逾時**：選舉逾時隨機範圍的下限，本專案為 400 ms（heartbeat 每 100 ms 一次）。

## 1. 為什麼要做

目前叢集固定是節點 1、2、3，多數固定為 2。這代表：壞掉一台只能修好它，不能換一台新的；也不能擴充到五台以容忍兩台故障。
博士論文 §4.5："fault-tolerance requirements can change over time, and failed servers eventually need to be replaced"。

這也是路線圖中風險最高的一段。依路線圖預先講好的刪減方案（§16）：若 S6 的整體強化無法通過，保留本規格與教學，把功能從最終版移除，叢集維持固定三台。

## 2. 來源

摘錄與頁碼見 `docs/reference/raft-notes.md`「成員變更」一節。

- **博士論文第 4 章**（2026-11-13 從 PDF 轉出的文字逐段讀過 §4.1、§4.2.1～§4.2.4、§4.4、§4.5；§4.3 joint consensus 只讀了開頭）：
  - §4.1：一次只增減一台時，新舊設定的任一多數必然重疊，可以直接切換。設定存在 log 中，
    "The new configuration takes effect on each server as soon as it is added to that server's log"，
    "each server always uses the latest configuration found in its log"。前一個變更 commit 之後才能開始下一個；
    log 中的設定 entry 可能因 leader 更換而被截掉，此時要退回前一個設定。
  - §4.2.1：新節點先以不投票的身分追趕。"The algorithm waits a fixed number of rounds (such as 10). If the last round
    lasts less than an election timeout, then the leader adds the new server to the cluster"，否則中止並回報錯誤；
    連不上或太慢也要中止（論文舉了 Paxos 的「議會全是溺水水手」，以及作者第一次變更就打錯埠號）。
    另建議 follower 在 AppendEntries 回覆中附上自己的 log 長度，讓 leader 直接調整 nextIndex。
  - §4.2.2：被移除的 leader 在 Cnew commit 之後才退位；在那之前 "it replicates log entries but does not count itself in majorities"；
    不在自己最新設定中的節點仍要發起選舉（Figure 4.6：從兩台叢集移除 leader），但
    "It does not count its own vote in elections unless it is part of its latest configuration"。
  - §4.2.3：被移除的節點收不到 heartbeat，"it will not receive the Cnew entry or learn of that entry's commitment"，會不斷以新 term 發起選舉。
    解法："if a server receives a RequestVote request within the minimum election timeout of hearing from a current leader,
    it does not update its term or grant its vote"；正常選舉不受影響，因為每台都至少等最小選舉逾時才參選。
  - §4.2.4：可用性論證，假設變更期間新舊設定各有多數可用。leader 能把 heartbeat 送到自己的設定時，
    它與 follower "will ignore any RequestVote messages with a higher term from other servers"。
    並指出退位的 leader "might become leader again"（風險很小，再當選後會再退位）——本規格 §9.2 消除這個情況。
  - §4.4：建議第一次建立叢集時，只讓一台節點以「只含自己」的設定作為 log 的第一筆，
    "it alone forms a majority of its configuration"；其他節點以空 log 啟動、經由成員變更加入。靜態設定檔 "potentially error-prone"。
    替換一台節點時應先加再減。
- **raft-dev 郵件**（Diego Ongaro，2015-07-10，"bug in single-server membership changes"）與所附 gist。本 session 沒有直接讀網頁的工具，
  由子代理讀取網頁並逐字引用；引文長度受限，反例的逐步狀態是子代理的轉述；網頁在 Jordan H. 的回覆之後被截斷，之後的回覆沒有讀到。
  - 原規則："A leader may only create a configuration entry in its log if the prior one is committed"，
    且 "the new one differs from the prior one by at most one server"。
  - 三個反例都從**四台**的叢集開始。發生條件："Starting from an even-sized cluster," "Multiple different changes are requested concurrently,"
    "Leadership is lost."；後果："data loss and permanent split brain could occur"。
  - 修正："a leader may not append a new configuration entry until it has committed an entry from its current term"；
    "This change would mean rejecting or delaying membership change requests until the no-op entry is committed"。
  - 理由："Once a leader has committed an entry in its current term, it knows it has the latest committed configuration,
    and no existing uncommitted configurations from prior terms can be committed anymore."
    Ousterhout 的說法（Diego 轉述）："if two configurations "compete", they differ by at most one server and hence have overlapping consensuses"。
  - "We don't yet have a formal safety proof for the correctness of membership changes (of either type) in Raft."；
    gist 是非正式的安全性論證。joint consensus 不受影響。

## 3. 範圍

**要做**

1. 設定存在 log 中，每個節點使用自己 log 中最新的設定；快照記錄它所在位置的設定。
2. 節點編號 1～7，設定可以是其中任意非空子集；多數 = 設定人數 ÷ 2 + 1（整數除法）。
3. 第一台節點引導、其他節點逐一加入（§17 D1）。
4. 一次只加入或移除一台；2015 年的修正；前一個變更 commit 之前不接受下一個。
5. 新節點先追趕再加入，以 heartbeat 次數計時（§17 D2）；連不上或太慢就中止。
6. leader 不在自己的設定中時，照常領導但不計自己；設定 commit 後退位。
7. 防止被移除的節點干擾（§17 D3）。
8. commit、選舉、ReadIndex 一律使用目前設定的多數。
9. 管理介面：增減節點的請求、狀態查詢中的成員資訊、命令列工具、操作說明。
10. 模擬器支援 5 個槽位與隨機成員變更；真實 process 測試、mutation、驗收階段 13、教學。

**不做**

- joint consensus、一次變更多台、leadership transfer、PreVote、check-quorum。
- 依故障自動增減節點（§4.4 提醒這需要明確的政策）。
- 更改既有節點的網路位址、動態發現叢集成員。節點與用戶端都依設定檔的位址表連線。
- 長期存在的「不投票成員」角色。不投票只是加入前的追趕狀態。
- 自動偵測操作錯誤（見 N2）。

## 4. 保證與不保證

**保證（都要有對應測試，見 §15）**

- G1 **同一 term 最多一個 leader**，在任何成員變更過程中都成立（模擬器 I1）。
- G2 **變更規則**：leader 只在以下三者同時成立時附加設定 entry：(a) 本任期已有 entry commit（2015 修正）；
  (b) 自己的 log 中沒有尚未 commit 的設定 entry；(c) 新設定與目前設定恰好差一台。
- G3 **已 commit 的 entry 不會遺失**，包括 2015 年反例的情境（模擬器 I4、I5，以及 §15 的反例測試）。
- G4 **設定在附加時生效**；log 被截短時自動退回前一個設定。
- G5 **追趕**：固定跑 10 輪；第 10 輪期間經過的 heartbeat 少於 4 次（即短於最小選舉逾時）才附加設定 entry，否則回報 `CATCH_UP_FAILED`。
  整個追趕超過 100 次 heartbeat（約 10 秒）也中止並回報 `CATCH_UP_FAILED`。中止時設定不變。
- G6 **leader 不在設定中**：leader 不在自己的目前設定中時繼續領導、不把自己算進多數；只要這份設定已 commit，就先回覆進行中的請求再退位。
  知道自己被移除的節點（最新設定不含自己，且 commit index 已達到該設定 entry）不發起選舉。
- G7 **不被干擾**：leader 能持續把 heartbeat 送到自己設定中的每個 follower 時，其他節點以更高 term 發出的 RequestVote，
  不會讓 leader 或 follower 更新 term，也不會讓 leader 退位。
- G8 **快照帶著設定**：快照記錄其位置的設定，InstallSnapshot 一併傳送；新節點可以只靠快照加上之後的 log 追上。
- G9 **ReadIndex 使用同一份設定**：回覆的投票成員不足以構成多數時，不確認任何回合（S3 留下的待辦）。

**不保證**

- N1 **沒有形式化證明**。Ongaro 本人在 2015 年也表示沒有；本專案的依據是論文、郵件中的論證與有限的測試。
- N2 **操作錯誤**：對兩台節點各自引導；清空某台的資料目錄後帶著 `--bootstrap` 重啟（等於另建一個叢集）；
  或把一個曾在**本任期**投過票的編號以清空的資料目錄重新加入。這些都可能破壞安全性，本版不偵測；操作說明會寫明正確做法。
- N3 變更期間需要**新舊設定各有多數可用**（§4.2.4 的假設）；否則可能暫時選不出 leader。
- N4 允許縮小到 1 或 2 台，但這時沒有容錯能力。工具會提示，不會阻止。
- N5 追趕以 heartbeat 次數計時，不是實際時間；機器很忙時，「4 次 heartbeat」可能長於 400 ms。
- N6 被移除的節點在操作者關掉之前會繼續執行。它通常不知道自己被移除（§4.2.3），會不斷以更高 term 參選；
  G7 讓它干擾不到叢集，但它仍佔用資源。
- N7 S3 以前建立的資料目錄沒有設定 entry；本版不支援，必須重新建立（路線圖決定只維護最後一版）。
- N8 把一個「被移除後仍在執行」的節點重新加入時，它的 term 可能已經很高；leader 收到它的回覆後會依 Raft 的規則退位一次，
  那次加入回覆 `NOT_LEADER`，重試即可。
- N9 追趕的 10 秒上限也包含快照傳送；很大的快照在很慢的機器上可能來不及，而中止後快照傳送會從頭開始，重試不一定會成功。

## 5. 資料模型

- `NodeId` 的有效範圍改為 1～7（`MAX_NODE = 7`）。core 內以節點編號為索引的陣列擴大為 8 格。
- 新型別 `Voters`：一組節點編號，內部為位元集合（第 n 位代表節點 n），提供包含、人數、多數、加入、移除、依編號走訪。
  合法的位元集合：非空，而且只用到第 1～7 位。
- 新的 log 命令 `Command::Config { voters }`：JSON 為 `{"Config":{"voters":[1,2,3]}}`，清單必須遞增、不重複、非空、皆在 1～7。
  從 WAL 讀出的 entry 與經由 AppendEntries 收到的 entry 使用同一個檢查。
- 某個 index 的設定 = log 中 index 不超過它的最新 `Config` entry；若 log 中沒有（已被壓縮），則為快照記錄的設定；兩者都沒有時為「無設定」。
- core 的**目前設定** = 最後一筆 entry 所在位置的設定。`DurableState` 另外記錄 log 中每筆設定 entry 的位置，推導時不必掃描整個 log。
- 快照的 `voters` 欄位：記錄快照位置的設定（S2 起已預留，原本固定為 `0b1110`），改為接受任何合法的位元集合。
  **快照檔與 WAL 的位元組格式不變**，`Config` 只是 Transaction JSON 中命令的新種類。
- 狀態機套用 `Config` entry 時只前進 applied index，不改變 KV（與 NoOp 相同）。
- `AppendResponse` 新增 `last_log_index`：follower 回報自己最後一筆的 index（§4.2.1 的建議）。
  失敗時 leader 的 `next` 改為 `min(next - 1, last_log_index + 1)`，仍不低於 `match + 1` 與 1。

## 6. 引導與節點啟動

- 新的啟動參數 `--bootstrap`：只對**完全空白**的資料目錄（沒有任何 log entry、沒有快照、term 為 0）生效，
  寫入 term 1 與一筆 index 1、term 1 的 `Config { voters: [自己] }`，經過正常的落盤流程後才開始服務。
  資料目錄不是空的時候忽略此參數並記錄一行 trace，因此同一台節點重啟時可以帶著相同的參數。
- 引導後，這台節點是自己唯一的投票成員。它在 term 2 參選；自己的一票在落盤後計入，此時已達多數（1），直接當選並 commit 它的 NoOp。
- 其他節點一律以空白資料目錄啟動，**沒有設定**：不發起選舉，只等 leader 聯絡；它仍會接受 AppendEntries 與 InstallSnapshot，
  也會投票（§4.1：處理 RPC 時不以自己的設定為準）。
- 設定檔改為**位址表**：`[[nodes]]` 可列出 1～7 中的任意幾台（編號與位址不重複）；若有 `[[routes]]`，必須涵蓋所列節點之間的每一個方向。
  節點自己的編號必須在位址表中；來自位址表以外編號的訊息一律丟棄。
- 測試用的叢集工具：引導節點 1、啟動其餘節點，再依序送出「加入 2」「加入 3」並等待完成。既有測試因此仍然面對一個三台的叢集。

## 7. 成員變更流程（core）

新事件 `Event::ChangeMembership { id, change }`（`change` 為 `Add(n)` 或 `Remove(n)`，`id` 由節點程式指定）與 `Event::CancelMembership { id }`；
新效果 `Effect::MembershipDone { id, voters, index }` 與 `Effect::MembershipFailed { id, status }`。leader 同時最多處理一個請求。

1. 不是 leader → `NOT_LEADER`。
2. 節點編號不在 1～7、或移除後設定會變成空的 → `INVALID_ARGUMENT`。
3. 已有進行中的請求，或 log 中有尚未 commit 的設定 entry → `CHANGE_IN_PROGRESS`（呼叫者稍後重試）。
4. 變更已經成立（加入已在設定中的節點、移除不在設定中的節點）→ 立即 `MembershipDone`。
5. 本任期尚未有 entry commit（2015 修正）→ 先等待，條件成立後再繼續。
6. `Remove(n)` → 附加 `Config(目前設定 - n)`。
7. `Add(n)` → 追趕 n（§8），成功後附加 `Config(目前設定 + n)`。
8. 設定 entry commit 後 → `MembershipDone`；若 leader 自己不在新設定中，接著退位（§9.3）。
9. 附加設定 entry **之前**失去 leader 身分或追趕失敗 → `MembershipFailed`（`NOT_LEADER`、`CATCH_UP_FAILED`），叢集沒有任何改變。
   **之後**才失去身分 → `TIMEOUT_UNKNOWN`：那筆 entry 仍可能被下一任 leader commit，和寫入的未知結果相同。
10. `CancelMembership`（節點程式的等待逾時）：忘掉這個請求、不再產生效果；若還在追趕就停止追趕。已附加的設定 entry 留在 log 中照常進行。
    節點程式自己回覆 `TIMEOUT_UNKNOWN`，與 S3 的 `CancelRead` 相同。

## 8. 追趕新節點

依 §4.2.1，用 heartbeat 次數代替時鐘（core 沒有時鐘；heartbeat 在節點程式中每 100 ms 一次，在模擬器中每 100 個虛擬毫秒一次）。

- 開始追趕時，把新節點的 `next` 設為 leader 的最後 index + 1、`match` 設為 0，並清掉它的進行中 RPC、快照傳送狀態與 ReadIndex 確認紀錄（和成為 leader 時的初始化相同）；
  之後它是複製對象，但不計入任何多數（commit、ReadIndex、選舉）。
- **一輪**的目標 = 該輪開始時 leader 的最後 index；新節點的 `match` 達到目標即完成，下一輪立刻開始。
- 第 10 輪完成時：這一輪期間經過的 heartbeat 少於 4 次 → 附加設定 entry；否則 `CATCH_UP_FAILED`。
- 從開始追趕起超過 100 次 heartbeat 仍未完成 10 輪 → `CATCH_UP_FAILED`（連不上、埠號打錯、太慢）。
- 中止時清掉該節點的複製狀態，之後不再送東西給它。
- leader 已壓縮掉新節點需要的 entry 時，照 S2 的流程送快照。

## 9. 設定如何影響既有行為

### 9.1 commit 與 ReadIndex

- commit：entry n 為本任期、且目前設定中 `match ≥ n` 的投票成員達到多數時 commit。leader 只有在自己是投票成員時才把自己算進去（match = 已落盤的 index）。
- ReadIndex：只計投票成員的確認；leader 自己只有是投票成員時才算一票；確認數不足以構成多數時，不確認任何回合（G9）。
- 為什麼在設定改變時仍然安全：計算時用的是 leader 當下的設定；任何兩份可能「互相競爭」的設定最多差一台（2015 修正所保證，見 §2 的 Ousterhout 引文），
  它們的多數必然重疊。所以構成多數的確認中，一定有一台也屬於任何可能選出新 leader 的多數；它若已經投給更高 term，回覆就會帶著更高的 term，不會被算成確認。

### 9.2 選舉

- **沒有設定的節點不發起選舉**。
- 候選人只向自己設定中的其他投票成員要票，只計算投票成員的票；自己的票在落盤後、且自己是投票成員時才計入。設定只有自己一台時，落盤當下即當選。
- 不在自己最新設定中的節點：只有在它**不知道**那份設定已 commit 時（`commit_index` 小於該設定 entry 的 index）才發起選舉，且不把自己的票算進去。
  這比論文 §4.2.2 嚴格一點。論文要求這種節點仍要參選，理由是 Figure 4.6（Cnew 尚未 commit 時，被移除的 leader 可能仍是唯一能推進的節點）；
  一旦知道 Cnew 已 commit，§4.2.2 也說 "it will always be possible for the members of Cnew to elect a new leader from among themselves"，
  它再參選只會造成 §4.2.4 提到的「退位後又當選」。不參選只影響活性，不影響安全性。
  節點重啟後 commit index 會回到快照位置，此時它不再「知道」，行為退回論文的版本，仍然安全。
- 收到 RequestVote：照論文 §4.1，不論候選人是否在自己的設定中都可以投票；但受 §9.4 的干擾防護限制。

### 9.3 leader 不在設定中

- leader 附加了不含自己的設定之後：繼續接受寫入與讀取、複製與管理叢集，只複製給設定中的成員，不把自己算進任何多數。
- **任何時候** leader 不在自己的目前設定中、且 `commit_index` 已達到該設定 entry：先回覆進行中的成員變更請求，再退位成 follower（term 不變）。
  這同時涵蓋「自己提出移除自己」與「在 Figure 4.6 的情況下再次當選」兩種情況。
- 退位時，等待中的讀取回覆 `NOT_LEADER`（與 S3 相同）；依 §9.2，它之後不再發起選舉。

### 9.4 干擾防護

- core 新增狀態「最近聽過 leader」：收到 term 不小於自己的 AppendEntries 或 InstallSnapshot（也就是承認對方是本 term 的 leader）時設為真，不論 log 檢查是否通過；
  成為 leader 時也設為真，並在 leader 期間維持（leader 一直聽得到自己）。
- 節點程式在每次 `ResetElection` 時，除了原本 400～800 ms 的隨機選舉逾時，另外排一個固定 400 ms 的 `Event::LeaderContactExpired { generation }`；
  generation 與目前的選舉 generation 相同時，core 把「最近聽過 leader」設為假。成為候選人時也設為假。
- 「最近聽過 leader」為真時收到 RequestVote：**不更新 term、不投票**，回覆拒絕（§4.2.3 允許丟棄、拒絕或延後，效果相同）。
- 正常選舉不受影響：每個節點至少等最小選舉逾時才參選，那時其他節點的「最近聽過 leader」也已過期。
  退位的 leader 會在退位後 400 ms 內繼續拒絕投票，與「最近聽過 leader（自己）」一致。
- 這條規則同時作用於原本固定三台的情境，所以既有的所有選舉測試都必須重新通過。

### 9.5 複製對象

leader 複製給：目前設定中的其他投票成員，加上追趕中的節點。節點不再是複製對象時（被移除、追趕中止），清掉 leader 為它保存的進行中 RPC 與快照傳送狀態；
之後它的回覆只用來檢查 term，不再更新任何複製狀態。

## 10. 管理介面

- 用戶端連接埠接受新的請求格式：`{"version":1,"membership":{"Add":4}}` 或 `{"version":1,"membership":{"Remove":2}}`。
- 回覆沿用現有的回覆格式：`status` 為 `OK`、`NOT_LEADER`（附 leader hint）、`CHANGE_IN_PROGRESS`、`CATCH_UP_FAILED`、`INVALID_ARGUMENT`、`TIMEOUT_UNKNOWN`；
  成功時 `result` 為新設定（例如 `"1,2,3,4"`），`committed_index` 為該設定 entry 的 index。
- 要加入的節點必須在位址表中，否則 `INVALID_ARGUMENT`（leader 沒有位址就無法追趕它）。
- 節點程式對管理請求的等待上限為 15 秒（追趕最多約 10 秒，再加上 commit 的時間），比一般請求的 3 秒長；逾時時送 `CancelMembership` 並回覆 `TIMEOUT_UNKNOWN`。
- 狀態查詢新增 `voters`、`config_index`、`config_committed`、`member`、`catching_up`。
- 新增命令列工具 `tools/kv_admin.py`（add／remove／status），依位址表尋找 leader；縮小到少於 3 台時印出警告。
- 操作說明寫進 `README.md`：如何引導、如何換掉一台故障的節點（先加後減，§4.4）、移除後再關掉節點、不要以清空的目錄重新加入同一個編號（N2）。

## 11. 儲存

- 位元組格式不變；`Config` 命令寫在 Transaction 的 JSON 中。快照檔的成員欄位改為檢查「合法的位元集合」。
- 快照時寫入快照位置的設定；安裝快照時一併採用。引導寫入的第一筆 entry 經由一般的落盤流程，不需要新的寫入路徑。

## 12. 模擬器

- 節點槽位擴大為 5（編號 1～5）。節點 1 引導，2～5 以空白狀態啟動；工作負載先依序加入 2、3，之後隨機提出加入或移除，
  設定人數保持在 1～5，並讓偶數大小確實出現（郵件："Starting from an even-sized cluster"；raft-notes 另有推導：三種組合都只在原設定為偶數台時，兩個競爭設定的多數才可能不重疊）。
- 原有的訊息遺失、延遲、重複、crash、分區照舊；分區改為隨機切開節點子集合。模擬器也要送 `LeaderContactExpired` 與追趕用的 heartbeat。
- 觀察者：I1～I8 照舊，需要「多數」的地方改用當時的設定。
- 新增 **I9 變更規則**：任何 leader 附加設定 entry 時，G2 的三個條件都成立。
- 新增 **I10 知道被移除就不參選**：最新設定不含自己、且 commit index 已達到該設定 entry 的節點，不會成為候選人。
- 報告新增：完成與失敗的變更次數、出現過的設定大小。
- 模擬器只檢查安全性，不檢查活性（教學 03 已記錄這個限制）。拿掉 2015 修正時模擬器能不能自己找到反例，實作時實測並如實記錄，不預先保證；
  確定能抓到的是 §15 的反例測試。

## 13. 和其他功能的關係

- **S2 快照**：快照必須記錄設定（論文 §7 已要求）；壓縮掉設定 entry 之後，設定只存在快照中。新節點可以經由快照追趕。
- **S3 ReadIndex**：多數改由目前設定決定；S3 的「不足就不確認」在這裡才能被測試觸發（G9）。
- **既有的 core 單元測試**：沒有設定的節點不再參選，所以原本以空 log 開始選舉的測試，改為以「index 1 是一筆 term 1 的三台設定 entry」開始
  （測試用的起始狀態；真實系統只會經由引導加逐一加入走到類似的狀態），這些測試的 term 與 index 因此各加一；
  原本以 NoOp 當第一筆的測試，改以設定 entry 當第一筆，編號不變。每一個修改都記錄在驗收契約的 `modified_tests`，原則照舊：新的不得較弱。
- **P00 的 `quorum_is_two_of_three`**：固定的多數不再存在，改由檢查每種設定大小的多數、拒絕重複與非法編號的新測試取代，記錄在 `replaced_tests`。
- **依教學 07 檢查「把行為當工具」的測試**：逐一檢查依賴「叢集如何啟動」「誰是第一個 leader」「log 從幾號開始」「固定三台」的測試，
  例如假設第一筆寫入是某個 index、假設初始 leader 隨機、以 `committed_index` 等待其他節點追上。結論記在 §18。
- **Mutation**：既有的 `quorum_one` 以 `QUORUM` 常數為錨點，S4 移除該常數，要改為植入「多數算錯」，記錄錨點的改變。

## 14. 實作順序

每一步結束時全部測試通過，並先用 `scripts/preflight_phase.py` 檢查新階段。

1. **protocol 與儲存**：`Voters`、`Config` 命令、節點編號 1～7、快照成員欄位、`AppendResponse.last_log_index`。狀態機套用 `Config` 為 no-op。
2. **core 讀取設定**：目前設定的推導與退回；commit、選舉、ReadIndex、複製對象都改用設定。既有測試改以三台設定開始，行為應與 S3 相同。
3. **core 變更流程**：§7～§9 的全部規則，含 2015 修正、追趕、leader 不在設定中、干擾防護；單元測試含 2015 年反例。
4. **節點程式**：`--bootstrap`、位址表、管理請求、`LeaderContactExpired` 計時、狀態欄位；Python 叢集工具改為引導加逐一加入，fault proxy 支援任意節點數；既有整合測試全部通過。
5. **模擬器**：5 個槽位、成員變更工作負載、I9、I10。
6. 真實 process 測試、mutation、驗收階段 13、教學、文件、操作說明。

## 15. 測試與驗收（階段 13）

**protocol／儲存**：`Voters` 的運算與每種大小的多數；設定的推導（log 中、被截短後退回、只在快照中）；`Config` 的格式檢查；
快照成員欄位接受任意合法集合、拒絕非法集合；含 `Config` 的 WAL 重啟後設定相同。

**core**：
- 引導的單一節點在自己的票落盤後當選並 commit；沒有設定的節點不參選。
- 加入：固定 10 輪、最後一輪夠短才附加設定；最後一輪太長時中止；連不上時在 100 次 heartbeat 後中止；中止時設定不變、不再送東西給它。
- 移除 follower；移除 leader（不計自己、commit 後才退位、退位後不再參選）；不在設定中卻當選的 leader 在設定已 commit 後退位。
- 有進行中的變更時拒絕其他變更；本任期未 commit 前不附加設定（2015 修正）；取消後不再產生效果。
- **2015 年的反例**：以五個 core 依郵件的第一個反例逐步送訊息（S1 把 `{1..5}` 只送給 S5 後離線；S2 當選後要求移除 S1，只把訊息送給 S3；之後 S1 回來參選）。
  同一份腳本：有修正時，S2 在 NoOp commit 前不會附加新設定，已 commit 的 entry 都沒有被覆蓋；拿掉修正時（mutation），已 commit 的設定 entry 會被 S1 覆蓋，測試失敗。
- 干擾防護：最近聽過 leader 時拒絕任何 RequestVote 且不更新 term；`LeaderContactExpired` 之後照常投票；leader 本身也拒絕。
- 非投票成員的票不計；不在設定中的候選人不計自己的票；知道自己被移除的節點不參選。
- ReadIndex：四台與五台的設定需要正確的多數；確認不足時不確認任何回合（G9）。
- `AppendResponse.last_log_index` 讓空白的新節點一個來回就定位。

**模擬器**：1,000 個種子 × 10,000 個事件，I1～I10 全部成立；報告中確實出現偶數大小、移除 leader 與追趕失敗的情況。

**真實 process**（Python）：
- 每個既有測試都經由引導加逐一加入啟動（等於持續測試加入流程）。
- 3 → 5 → 3 的成長與縮小，期間持續寫入與讀取，歷史可線性化。
- 移除 leader：新 leader 從其餘節點中產生，寫入恢復，被移除的節點停止參選。
- 以快照追趕：先讓 leader 壓縮過，再加入新節點。
- 加入一個連不上的節點（位址表中有、但沒有啟動）：回覆 `CATCH_UP_FAILED`，設定不變。
- 被移除但仍在執行的節點：一段時間內叢集的 leader 與 term 不變。
- 換掉一台故障的節點：殺掉節點 3、加入 4、移除 3，叢集持續服務。
- 五台叢集的 fault proxy 涵蓋全部 20 個方向。

**Mutation**（至少）：拿掉 2015 修正；允許同時兩個變更；計入非投票成員的票；leader 不在設定中仍把自己算進多數；設定 commit 後不退位；
拿掉干擾防護；不追趕就加入；ReadIndex 仍用固定的多數；`quorum_one` 改為「多數少算一票」。

**Bug 教學**：實作中遇到的每個 bug 寫成教學。

## 16. 刪減方案

照路線圖：若本功能無法通過 S6 的整體強化（模擬器長時間執行、故障注入下的線性一致性、mutation、獨立審查），
就保留本規格、教學與測試紀錄，從最終版移除成員變更，叢集回到固定三台。這會是一次明確的決定與 commit，不會悄悄停用。

## 17. 已確認的決定（2026-11-13）

| 編號 | 問題 | 決定 |
|---|---|---|
| D1 | 叢集第一次建立時的成員名單 | 依博士論文 §4.4：一台以「只含自己」的設定引導，其他節點以空 log 啟動、逐一加入 |
| D2 | 新節點加入前是否追趕 | 依 §4.2.1 先追趕再加入：固定 10 輪，最後一輪短於最小選舉逾時（少於 4 次 heartbeat）才加入；總上限 100 次 heartbeat |
| D3 | 被移除節點的干擾 | 依 §4.2.3 加上防護，以固定 400 ms 的 `LeaderContactExpired` 事件判斷「最近聽過 leader」 |
| D4 | 節點編號與規模 | 編號 1～7，最多 7 台；模擬器使用 5 個槽位 |

D1～D4 的方向由使用者在問答中選定；輪數、heartbeat 次數與 §9.2 的「知道被移除就不參選」是本規格的提案，隨規格一併核准。

核准後 `AGENTS.md` 在同一個 commit 修改：「fixed IDs 1/2/3 and quorum 2」改為節點編號 1～7、設定取自 log 中最新的設定 entry、多數為該設定的多數（一次一台，見本文件）；
解除「No ... membership changes」；路線圖表格的 S4 列標示已核准。

## 18. 實作補記

（實作完成後填寫：實際做法與本規格的差異、遇到的 bug 與教學、教學 07 式的測試檢查結論、驗收證據。）
