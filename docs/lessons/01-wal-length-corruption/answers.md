# 第 9 節解答

## 理解題

**1. 長度改成 3,000,000 會怎樣？**
舊版會正確地拒絕啟動，錯誤是 `WAL record length out of bounds`。因為 `check_header` 有檢查長度必須介於 1 和 2 MiB（2,097,152）之間，
3,000,000 超過上限。這說明舊版不是完全沒檢查長度，而是只檢查了「範圍」，沒檢查「是不是原本寫進去的那個值」。
65536 落在合法範圍內，所以溜過去了。（實測：2026-11-12 以長度 2,097,153 執行，結果如上。）

**2. 長度被改小時，舊版在哪一步發現？**
在驗整筆 record 的 CRC 時。長度是 90 的話，程式會讀 90 bytes 當 payload，把接下來的 4 bytes 當成 CRC。
那 4 bytes 其實是「payload 的最後一個 byte 加上真正 CRC 的前三個 bytes」，和算出來的值對不上，於是回報 checksum mismatch。
重點是：這條路徑上程式**把整筆讀完了**，所以 CRC 有機會發揮作用。長度改大到超出檔案時，程式在讀完之前就先判定為殘尾，CRC 根本沒被用到。

**3. 把新版的兩步順序對調，案例 A 會怎樣？**
會退回舊版的行為：程式先看到「長度 65536 超過檔案剩餘」，判定為殘尾並截斷，header 的 CRC 還來不及驗。
多了一個 CRC 欄位並不會自動帶來保護，**在使用之前先驗證**才是關鍵。
`scripts/mutation_check.py` 裡的 `wal_compare_eof_before_header_checksum` 就是植入這個錯誤，案例 A、B 的測試會因此失敗。
（新版因為截斷前會先存副本，資料還找得回來，但節點仍然會帶著被截短的 log 啟動。）

**4. 為什麼半筆 record 可以放心刪掉？**
因為程式的規則是：整筆寫完，並且 `sync_all` 成功之後，`save` 才回傳成功；上層只有在 `save` 成功後才會送出投票或確認。
半筆 record 代表 `save` 還沒回傳，所以沒有任何人收到過依賴這筆資料的回覆，刪掉它不會讓節點「說話不算話」。
如果程式改成先回覆、再寫磁碟，這個推論就不成立了。

**5. 為什麼拒絕啟動比較安全？**
三台裡停一台，剩下兩台仍是多數，叢集可以繼續服務，而且那兩台的記憶是完整的。
一台帶著被截斷的 log 繼續運作的節點卻會參與投票，它可能把票投給本來不該當選的人（它忘了自己已經投過，或它的 log 變短讓別人顯得夠新），
結果是同一任期出現兩位 leader，或已定案的資料被覆蓋。停下來只損失一點容錯能力；帶著錯誤的記憶繼續跑，損失的是正確性。

## 動手練習

把 `check_header` 裡的條件改成永遠不成立之後，實際執行 `cargo test -p raft-storage` 的結果（2026-11-12）：

```text
test tests::reproduced_length_corruption_fails_closed_unchanged ... FAILED
test tests::every_single_bit_flip_fails_closed_unchanged ... FAILED
test tests::seeded_multibyte_corruption_never_accepts_or_shortens ... FAILED
byte 89 bit 1 was accepted
test result: FAILED. 19 passed; 3 failed; 0 ignored; ...
```

三個測試失敗，其餘十九個通過。逐一翻轉的測試回報第一個被接受的是 **第 89 個 byte 的 bit 1**。

對照規格：檔頭佔 0～79，第一筆 record 從 80 開始，長度欄位在相對位置 8～11，也就是檔案的第 88～91 個 byte。
第 89 個 byte 是長度的第二個 byte（little-endian，權重 256），翻轉它的 bit 1 等於把長度加上 512。

為什麼前面的翻轉都沒出事？

- 第 80～87 個 byte 是 magic、版本、類型。它們的值是固定的，即使沒有 CRC，欄位比對也會擋下。
- 第一筆的長度是 168，十六進位 `a8 00 00 00`。翻轉第 88 個 byte 的某個 bit，長度會改變 1 到 128：
  原本是 0 的 bit 會讓它變大，原本是 1 的 bit（第 3、5、7 個）會讓它變小。
  變小時程式讀到的「CRC」其實是 payload 的尾巴，對不上，拒絕。
  變大的幅度最多 64（168 + 64 = 232），第 89 個 byte 的 bit 0 則讓它變成 168 + 256 = 424。
  測試用的檔案有三筆 record，從第一筆開頭算起還有將近 600 bytes，所以這些長度仍落在檔案範圍內；
  程式照著錯的長度讀完一筆，整筆的 CRC 不符，拒絕。
- 第 89 個 byte 的 bit 1 讓長度變成 168 + 512 = 680，加上 header 與 CRC 是 708 bytes，超過檔案剩下的大小。
  這是第一個「宣告的長度超出檔案剩餘」的翻轉。程式不再讀完整筆，而是把它當成殘尾截斷——和舊版的 bug 一模一樣。

（168 這個數字取自測試檔第一筆 Transaction 的 JSON 長度；「將近 600 bytes」是三筆 record 的總長，
可以在練習用的副本裡印出 `sample.bytes.len()` 自行確認。）

這正好重現了第 4 節說的現象：長度的毀損只有在「大到超出檔案」時才繞得過整筆的 CRC，所以需要 header 自己的 CRC 來擋。
