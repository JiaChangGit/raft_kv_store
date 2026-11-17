# 工程強化：修正、重現與交付邊界

> **V1 時期的紀錄。** 「WAL v1 仍有的格式限制」一節，在 S1 改用 [WAL 格式 2](STORAGE_FORMAT_V2.md) 時已經解決。「從乾淨副本重建」描述的 `scripts/clean_rebuild.py` 仍在，但 S9 沒有重跑它。

本階段依使用者指示接續教學驗收，範圍是獨立審查、負向測試與乾淨副本重建。
沒有加入 V2 功能。變更前的程式及新增紅測試
保存在 `artifacts/audit/engineering-baseline.zip`；實際結果以本機原始輸出及最新 receipt 為準。

## 已查證的問題與修正

| 問題與觸發 | 修正及驗證 |
|---|---|
| 已使用的節點保留 node.meta、卻缺少 raft.wal；原程式重建空 WAL | 既有 identity 只允許開啟既有 WAL，不再 create_new。`existing_identity_without_wal_fails_closed` 先失敗再通過，且確認沒有產生新空檔 |
| 完整但未 sync 的交易在 crash 後存活；回復直接當 durable 基線，可對同 term 同候選人產生 grant 效果而沒有 Persist | 回復成功前再 sync_all，失敗則不服務。注入同步失敗的回歸與實際 fsync 計數互補：修正前 recovery probe 為 0 次，修正後為 1 次；這是 core grant 效果觀察，不冒稱 TCP ACK 或斷電實驗 |
| reorder 等待第二筆 frame 時逾時，取消已消耗部分 bytes 的 coroutine；後續 payload 被當成 header | 保留同一 read task，held-frame flush 只改變發送時點。慢速分段第二筆的真實 socket 回歸可完整轉送兩筆 frame |
| 未知 status 被略過，NaN 破壞先後條件，多個 operation variant 被默默取第一個 | 嚴格檢查 status、有限時間、u128/u64、單一操作、字串欄位及結果。無效 history 明確拒絕，不回 PASS |
| STORAGE_ERROR 被當作確定未執行 | 與 TIMEOUT_UNKNOWN 同樣容許未發生或合法完成；NOT_LEADER 依目前前置拒絕契約視為無效果。空有效 history 回 INCONCLUSIVE，因它沒有實驗證據，並非宣稱數學上不可線性化 |
| source snapshot 略過 file/dir symlink；report 的 command/metrics/environment 型別錯誤仍過關 | 未排除的 source symlink 拒絕驗收，錯誤寫 FAIL receipt；報告檢查型別及 simulation 計數一致性。既有 discovery/執行數量檢查保留 |
| outbound pool 滿時排入立即到期的 retry，可能持續優先處理 timeout 而餓死 status | 改為有界 backoff。直接構造滿額 actor 的回歸確認 queued status 仍能得到回覆；未宣稱已以 128 條真實慢連線驗完整可達路徑 |
| socket timeout 每次 partial read 重新計時，慢速串流可長期占用 worker | 加入 connect/write/read 共用的 outbound deadline；每個接收 frame 也有總讀取期限。真實慢速分段 TCP 測試在總期限內退出 |
| follower 停止後立即 EOF／拒絕連線會馬上重送，已量到一秒上千次 retry | 失敗通知保留原 request deadline，到期才淘汰及重試。真實三程序負向測試先重現暴增，再確認 retry 有界且多數可繼續提交 |

紅測試與重現資料在 `artifacts/audit/`，其中 `oracle-gate-red.txt`、
`wal-missing-red.txt` 及各子目錄的 results.json／trace 保留首敗。
後續成功不覆寫這些失敗紀錄。
既有 mutation helper 也加入「滿額後立即重試」的錯誤副本，確認 queued status 回歸能
抓到忙迴圈；只修改隔離副本，不改正式程式或驗收條件。

## WAL v1 仍有的格式限制

WAL v1 沒有獨立 header checksum。已實際重現：把中間 record 的 payload_len 改成
合法上限內、但超過 EOF 的數字，可能被當成 incomplete tail 而截斷。這是任意 header
毀損情境，不能混同正常 process crash 的 append-prefix 模型。文件已改成精確描述
可辨識的 CRC 錯誤，沒有假稱程式能判斷所有損壞，也沒有無聲改寫格式。

改善這個限制需要規格、版本識別、舊資料處理與遷移驗收，列為 V2 設計候選。
缺 WAL 的初始化中斷也保持不可用，避免為了可用性猜測資料是空白。

## 從乾淨副本重建

在 WSL 專案根目錄，Rust 工具已可用後執行：

```bash
.venv/bin/python scripts/clean_rebuild.py --output-dir artifacts/clean-build/my-first-run
```

輸出目錄須尚不存在。腳本核對來源 hash，複製一般來源檔到新目錄，建立全新 target
和 Python venv，安裝宣告依賴，執行 locked build／clippy／完整 Rust 與 Python 測試，
最後用副本內的實際 CLI 跑三程序 crash/restart 示範。所有命令與 stdout/stderr 都保存。
它不帶入原本的 target、venv、舊 receipt 或舊測試資料。

這仍使用同一主機的 Rustup、Python 與 package caches；不是全新 OS、冷 cache 或離線驗收。
新副本的 framework 本機配置也未複製；原生 build/test/CLI 路徑須能獨立運作。
本機結果檔在 `artifacts/clean-build/`；失敗會保留副本，不能只看目錄已建立就說成功。

## 驗收工具版本與交接

這次加強了 checker，原始 acceptance/v1.json 的必要測試與條件沒有刪減。
checker SHA 改變後必須從 P00 重新依序驗收，並在來源穩定時執行；舊 receipt 只代表
當時版本。每次 run 都有獨立資料夾，最後以最近嘗試和當前 fingerprint 核對。

本階段的完成結果另存 `artifacts/reports/engineering-stage2.json`。來源可能隨後續工作
改變，接手者仍須回到實際檔案與記錄。真人理解、任意硬體斷電、所有腐損情境與形式化
協定證明仍未完成；有限測試不能取代它們。

Windows 原生 Python 也執行 checker 的 7 個單元案例：5 個通過，2 個因目前 UNC 路徑
無法建立 symlink 而跳過；輸出保存在 `artifacts/audit/windows-gate-unit-stage2.txt`。
同樣的 symlink 正負情境在 WSL 實際執行通過。這只驗 checker 的部分跨平台行為，
不代表 Rust runtime 已在 Windows 驗收。
