# 自問自答

讀完 [架構說明](ARCHITECTURE.md) 和 [帶讀](LEARNING_GUIDE.md) 之後，用這些問題檢查自己懂了沒有。先自己回答，再看答案。
每個答案後面附上可以驗證它的地方：文件、教學或測試名稱。看不懂的名詞查 [名詞表](GLOSSARY.md)。

## 基本

**Q1. 為什麼寫入只要多數同意，不用全部？**
任兩個多數至少有一台重疊。一筆 entry 只要在多數節點落盤，之後要當選的 leader 就必須拿到多數的票，這些票裡至少有一票來自存有這筆 entry 的節點。
投票規則不讓 log 比較舊的節點當選，所以新的 leader 一定也有這筆 entry，它不會丟。要求全部同意的話，一台壞掉整個叢集就停了。
→ Raft 論文 §5.4；`Voters::is_quorum`。

**Q2. 三台的叢集可以壞幾台？兩台呢？**
三台的多數是兩台，可以壞一台。兩台的多數也是兩台，任何一台壞掉都會停，所以 `kv_admin.py add 2` 成功後會印一行警告。
五台可以壞兩台。→ `Voters::majority`；README 的快速開始。

**Q3. term 是做什麼用的？**
它是「現在是第幾任」。同一個 term 最多一個 leader；節點看到比自己大的 term，就知道自己過時了，立刻退回 follower。
舊 leader 被隔離後回來，它送出的訊息帶著舊的 term，會被拒絕。→ 論文 §5.1；`Core::step_down`。

**Q4. 為什麼選舉逾時要隨機？**
如果大家同時逾時、同時拉票，票會被分散，誰都拿不到多數，只好再選一次。隨機的逾時（這裡是 400～800 毫秒）讓通常只有一台先開始。
→ 論文 §5.2；`Actor::reset_election`。

## 寫入

**Q5. 一筆 PUT 什麼時候算成功？**
leader 把它寫進 log、自己落盤、送給 follower；多數落盤後 leader 把它 commit，依序套用到狀態機，才回覆 OK。
回覆時，這筆寫入已經在多數節點的磁碟上。→ [架構說明](ARCHITECTURE.md)「一次寫入」。

**Q6. 為什麼一定要「先落盤，再送訊息或回覆」？**
回覆或投票是一個承諾。如果節點先說「我收到了」或「我投給你」，還沒寫進磁碟就當機，重開後它不記得這個承諾：
可能在同一個 term 再投給別人（出現兩個 leader），或讓 leader 以為某筆 entry 在多數上、其實不在。
所以 core 每一步只產生一個 barrier（「這些變動落盤之後，才能做後面的事」的關卡，見 [名詞表](GLOSSARY.md)），排在所有訊息之前。→ `Core::seal`；教學 01、09。

**Q7. 為什麼 leader 不能直接 commit 舊 term 留下的 entry？**
即使舊 entry 已經在多數節點上，它還是可能被之後的 leader 蓋掉（論文 Figure 8 的劇本）。leader 只直接 commit 本任期的 entry；
本任期的 entry 一 commit，它前面的 entry 也就一起 commit 了。這也是新 leader 當選後先寫一筆 NoOp 的原因。
→ [commit 規則](COMMIT_RULE_REVIEW.md)；測試 `previous_term_entry_requires_current_term_commit`。

**Q8. 回覆在路上遺失了，client 重送，會不會寫兩次？**
不會，只要重送時用同一組 client ID、sequence 與內容。狀態機記得每個 client 最近一筆寫入的結果，看到同一組就直接回傳舊的結果。
換一個新的 sequence 就是另一個請求了。→ [client 契約](CLIENT_CONTRACT.md)；測試 `retry_same_request_has_single_effect`。

**Q9. group commit 讓多筆寫入共用一次 fsync，會不會比較不安全？**
不會。每筆寫入仍然先進 log、和這一步的其他變動一起落盤，然後才可能被 commit 和回覆；改變的只是「一次 fsync 涵蓋幾筆」。
當機時，這一批要嘛全部在磁碟上，要嘛都還沒被回覆。→ [效能規格](PERFORMANCE.md) §6；測試 `test_a_crash_right_after_a_batch_of_writes_executes_each_once`。

## 儲存

**Q10. 為什麼 WAL 每筆 record 的 header 要有自己的檢查碼？**
回復時要先讀 header 裡的長度，才知道這筆有多長。如果長度欄位壞了又沒有檢查，程式可能把後面完好的資料誤當成「沒寫完的尾巴」而刪掉，
被刪掉的資料裡可能有節點投過的票。所以長度在通過 header 的檢查碼之前，不拿來做任何決定。→ [教學 01](lessons/01-wal-length-corruption/README.md)。

**Q11. 寫到一半當機，檔案尾巴不完整，怎麼辦？**
只有能確定是「沒寫完」的兩種尾巴會被截掉，截掉前先存一份副本。其他任何不符都拒絕啟動（fail closed），不猜。
→ [WAL 格式 2](STORAGE_FORMAT_V2.md) §5。

**Q12. log 被壓縮之後，落後很多的 follower 怎麼追上？**
leader 要送的 entry 已經被刪了，只能改送整份快照（InstallSnapshot）。這兩個功能必須一起上線：只有壓縮、沒有 InstallSnapshot 時，
第一個落後的 follower 就讓 leader 停住。→ [教學 03](lessons/03-compaction-without-install-snapshot/README.md)。

## 讀取

**Q13. 讀取為什麼不能直接讀 leader 的狀態機？**
自以為是 leader 的節點可能已經被隔離，另一邊已經選出新 leader 並寫入了新的值。它直接回答就會讀到舊值。
所以 leader 回答前要先送一輪訊息，確認自己仍被多數承認。→ [ReadIndex 規格](READ_INDEX.md)；測試 `test_a_leader_left_with_a_minority_does_not_answer_reads`。

**Q14. ReadIndex 為什麼要等 leader 在本任期 commit 過一筆？**
剛當選的 leader 不知道之前 commit 到哪裡（它的 commit index 可能落後）。等本任期的 NoOp commit 之後，它的 commit index 才至少涵蓋所有已 commit 的寫入。
→ READ_INDEX.md §6；測試 `read_waits_for_a_committed_entry_of_its_term`。

**Q15. 為什麼 GET 不去重？**
GET 不改資料，讀兩次沒有副作用；而且 S3 起 GET 不進 log，也就沒有東西可以去重。重送同一個 GET 就是再讀一次，可能讀到更新的值，這仍然是線性一致的。
→ [帶讀](LEARNING_GUIDE.md) 的練習 D。

**Q16. 只有一台的叢集，讀取為什麼曾經卡住？**
回答讀取的判斷只在「收到 follower 回覆」或「落盤完成」時才被執行。一台時沒有 follower，閒著時也沒有落盤，判斷就永遠不被執行。
→ [教學 20](lessons/20-a-leader-alone-never-answers-reads/README.md)。

## 成員變更

**Q17. 為什麼一次只能加或移除一台？**
一次只差一台時，舊設定的多數和新設定的多數一定重疊，不會出現兩邊各自選出 leader 的情況。一次改多台需要 joint consensus，規則複雜得多。
→ [成員變更規格](MEMBERSHIP.md)；論文第 4 章。

**Q18. 新節點為什麼要先追趕，才寫入新的成員設定？**
空的節點一加入就算進多數。三台加成四台時多數是三台，如果新節點要很久才追上，這段時間只要再壞一台就無法寫入。
所以 leader 先讓它追上 log（固定 10 輪，最後一輪夠短才算追上），才寫入新的設定。→ MEMBERSHIP.md §8。

**Q19. 新 leader 為什麼要先在本任期 commit 一筆，才處理成員變更？**
單步變更原本有一個反例：新 leader 不知道上一任留下的設定是否已 commit，就再寫一份新的，兩份設定的多數可能不重疊。
2015 年補上的規則是先 commit 本任期的一筆 entry。這個專案還因為「太早判斷」踩過一次 bug。→ [教學 08](lessons/08-judged-before-the-term-commit/README.md)。

**Q20. 為什麼不能把清空資料的節點用同一個編號加回去？**
清空的節點忘了自己投過的票和曾經確認過的 entry。它可能在同一個 term 再投一次票，或讓 leader 以為某筆 entry 還在多數上。
這一版偵測不到這種操作錯誤，所以 README 要求換一個新的編號加入，再移除舊的。→ README 的快速開始。

## 測試與驗收

**Q21. 模擬器和真實節點用同一份 `raft-core`，好處是什麼？**
模擬器找到的 bug 就是真實節點的 bug，不是另一份實作的 bug。core 不碰時鐘、網路和磁碟，模擬器才能用虛擬時間、注入故障，並用種子重跑出完全相同的過程。
→ [架構說明](ARCHITECTURE.md)「元件」。

**Q22. mutation 測試證明了什麼？沒有證明什麼？**
它證明：被列出的每一個植入錯誤，都至少讓一個測試失敗。它沒有證明：沒被列出的錯誤也會被抓到，或測試的斷言夠強。
S10 複查時，審查者在 30 個地方植入錯誤，有 16 個沒有測試抓到。S10 補了其中 12 個的測試；另外 4 個別處已重複檢查或影響很小，S12 也補上了。→ `scripts/mutation_check.py`；[開發歷程](DEVELOPMENT.md) S10、S12。

**Q23. 測試全部通過，就代表沒有 bug 嗎？**
不代表。教學 18 裡所有測試都通過，落後超過 32 筆的 follower 卻永遠追不上；教學 20、21 的 bug 也一直沒被測試抓到，
因為測試都用三台、都在一個已經跑過測試的目錄裡。測試只能證明它們檢查的情況。→ [教學 18](lessons/18-a-limit-nobody-tested/README.md)。

**Q24. 這個專案沒有驗證哪些事？**
沒有形式化證明；沒有長時間的浸泡測試；故障注入下的線性一致性檢查一次最多 24 個操作；SIGKILL 不等於斷電；效能數字只來自一台機器。
完整清單在 README 的「沒有驗證的事」。

**Q25. SIGKILL 和斷電差在哪裡？**
SIGKILL 只終止 process，作業系統還在，已經交給它但還沒同步的資料仍可能寫到磁碟。斷電時連作業系統也停了，沒同步的就沒了；
有些磁碟甚至在 fsync 回報成功後還暫存著資料。這個專案假設 `sync_all` 成功就代表資料已寫到磁碟，沒有測斷電。

## 設計

**Q26. 為什麼用一個 actor 執行緒擁有全部狀態，而不用鎖？**
共識的狀態（term、投票、log、commit index）彼此牽連，用鎖保護很容易漏掉一個組合。一條執行緒依序處理事件，就不需要鎖，
也保證「先落盤、再送訊息」的順序不會被別的執行緒插隊。其他執行緒只負責收發 frame。→ `Actor::run`。

**Q27. 為什麼不用現成的 Raft 函式庫？**
這個專案的目的就是學會 Raft 怎麼運作、會在哪裡出錯。用現成的函式庫，那 21 個 bug 一個都不會遇到。
