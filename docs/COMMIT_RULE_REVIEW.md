# Commit rule 與 Figure 8

> **V1 時期撰寫（當時固定三個節點）。** Figure 8 與「只直接提交本任期 entry」的推導仍然適用，函式仍是 `Core::advance_commit`；但「三個固定節點」「至少兩個節點的 durable match」在 S4 之後改成「目前設定的多數」（[成員變更規格](MEMBERSHIP.md)）。

來源：Raft extended paper Figure 2、Figure 3、§5.4.2、Figure 8；本專案原始
ALL_IN_ONE_SPEC.md 的 P02。正式設定始終三個固定節點；論文 Figure 8 的五節點
只用作反例推理，沒有變更正式拓樸。

`Core::advance_commit` 從本地 durable_index 向下找 N，要求
`log[N].term == currentTerm` 且至少兩個不同節點的 durable match >= N。
本地 match 只在 `persisted(barrier_id)` 成功後增加。收到 follower success 時，
Core 核對 RPC ID、term、leadership epoch 與該次 prev+count；遠端任意 last index
不能取代本次確認 prefix。已 retired request 的 success/failure 不改狀態。

Figure 8 的重點：舊 term entry 即使暫時位於多數節點，後續更高 term、較新 log
的候選人仍可能獲票並覆蓋它。不能把這個副本數當成直接提交條件。當前 term
entry 經 quorum 提交後，Log Matching 與 election freshness 使前面的合法 prefix
一併間接提交。這是協定條件，不是可移除的保守效能選項。

`previous_term_entry_requires_current_term_commit` 的獨立 fixture 先建立舊 entry
在兩個 durable matches、當前 term NoOp 只在本地的狀態：commit 必須維持零。
再讓當前 term entry 也取得 quorum，才依序 Apply 舊 prefix 與 NoOp。
另有 RAM-only self match 的反例。這些有限測試與推理不是實作形式化證明。

Follower 的 commit 上限是本次 AE 確認的 prev+count；空 heartbeat 不確認自己
多出的 suffix。commit 不倒退，last_applied 依序增加且不超過 commit。所有 log
變更與回覆之間都受 leading Persist barrier 約束。
