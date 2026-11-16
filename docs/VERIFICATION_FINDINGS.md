# 驗證結果與界線

> **歷史文件：V1 的驗證紀錄。** 它對應已停用的 `acceptance/v1.json`。目前有效的是[最終驗收契約](ACCEPTANCE_FINAL.md)；之後各階段的發現寫在各自的規格補記，以及 [S1～S4 的獨立審查](INDEPENDENT_REVIEW_S1_S4.md)與 [S5 的獨立審查](INDEPENDENT_REVIEW_S5.md)。

本專案使用 alternative-acceptance-v1，原始 gate 資產不可取得。最新階段結果不手抄成
固定 PASS 表：請讀 `artifacts/acceptance-v1/phase-XX.last-attempt.json`，核對 status、
source fingerprint、contract/checker SHA、命令及 evidence_directory。舊成功 receipt
不能蓋過較新的失敗。`scripts/status.py` 可列最新狀態。

## 實際執行的工程驗證

- Rust core 的具名選舉、複製、commit、GET intent、慢 ACK 回歸已執行通過。
- WAL/KV/transport/node 的單元測試與真實 storage subprocess crash 測試已執行通過。
- Python history/proxy 正反案例已執行；checker 拒絕 stale read/矛盾結果，允許 unknown
  write 合法存在或不存在，保留 real-time order、session 與序號語意。
- 三個獨立 Linux PID 的選舉、SIGKILL/restart、全部重啟、client retry、commit後reply前
  crash、隔離舊 Leader、heal、route檢查、sync failure 無成功投票回覆已實跑。
- 實際 peer 連線測試 fragmented frames 與同連線多個完整 request；client測不同版本、
  缺版號、未知欄位、UTF-8 byte bounds 與完整 u128 ID。
- 1000 seeds × 10000 events 的實際 release sim 結果在
  `artifacts/reports/verification.json`，原始命令輸出在 `artifacts/runs/sim-release-final/`。
  不變條件 observer 讀取獨立 durable vote/log 與歷史 committed/applied maps，沒有第二套選舉。
- 真實 process benchmark 在 `artifacts/reports/benchmark.json`，原始樣本及node traces
  留在該報告目錄對應 `artifacts/runs/`。數字只適用該次執行。
- 隔離 mutation 的 quorum=1、移除 current-term predicate、反轉queue三個錯誤版本，
  都由相關測試實際失敗抓到。結果在 `artifacts/mutations/*/report.json`，正式程式未改。

最後每項實際 pass/fail/count 與版本仍應回到 receipt、JUnit 與原始輸出核對；
上列工作類型不表示尚未產生的階段 receipt 已存在。

## 保留的發現與修正

1. P00 審查發現只有 schedule/pop，缺顯式 fault hooks；補 drop/delay/duplicate hooks、
   獨立順序斷言與 hook測試。
2. 心跳每100ms淘汰尚未逾時AE會讓150ms合法ACK永遠過期；改成匹配RpcTimeout才淘汰，
   同時保留monotonic deadline，加入慢ACK回歸。
3. runtime Leader退位後可能沒有election timer；只補缺少timer，不重置既有Follower期限。
4. proxy reorder暫存frame在新partition規則後flush會洩漏；flush也重查最新規則。
5. sim觀察點原先可能漏掉當選後立刻crash；新增effect執行前的leader transition observer，
   ACK亦比對durable term與prefix，並對齊runtime退位timer政策。
6. benchmark首次smoke在restart後立即query而ConnectionRefused；改先取得存活多數的
   target commit，再poll重啟節點到達目標。首次失敗trace在
   `artifacts/runs/test_benchmark_smoke-9f224355fd/`，修正後另跑smoke與量測。
7. 工具版本升級的deprecated atomic API與三項clippy警告按實作修正，未加allow或降lint。
8. 一次未優化的完整debug campaign為執行效率中止，不能算PASS；test profile改為opt-level2，
   測試數量仍1000×10000，之後以實際完整結果驗收。
9. 完整Python回歸曾因route測試把「安裝六條規則期間」的合法allow frame也算入而失敗。
   觀察起點改到六個同步rule callback全部完成後，仍要求之後每個frame都是drop且新請求
   無法成功。首敗在`artifacts/runs/test_fault_proxy_does_not_bypass_route-4f513d20a7/`。
10. 獨立reference model補齊byte bounds的INVALID_ARGUMENT語意；正例允許拒絕超限輸入，
    負例拒絕把它標成成功效果。來源改動使當時驗收fingerprint失效，完整鏈重新執行。
11. P08補上sync_all呼叫本身計時，與整段Wal.save分開報告，並驗兩種樣本數一致；
    原P08因來源改動失效後重跑。最後P08累積重驗全部前階段要求；前階段receipt仍代表
    當時來源，不假稱所有階段fingerprint相同。
12. 完整回歸的unknown-outcome測試曾在舊Leader重啟後仍排除它，合法重新當選會導致
    假逾時。先在舊節點停止時確認其他節點當選，再重啟並重試，且斷言只有一次效果。
    首敗原始輸出在`artifacts/runs/python-suite-fsync/`；當時三節點KV已一致，屬測試時序
    錯誤，不冒稱協定安全性失敗或以增加timeout掩蓋。

## 尚未支持的結論

沒有形式化實作證明、任意硬體斷電測試、壞碟保證、多主機網路benchmark、長期資料庫運維
或真人人類學習效果。process tests只用有限seed/workload；checker搜尋界限24操作，超限為
INCONCLUSIVE，不能PASS。未知操作不一律當失敗，永久不穩定網路不要求固定時間進展。

V1 原始驗收時 Mermaid 僅保留圖源，當時沒有原生渲染或視覺驗收。後續教學階段已在隔離
目錄安裝 Mermaid CLI 並渲染兩張圖，詳見 [帶讀](LEARNING_GUIDE.md)；這不改變當時的驗收紀錄。
proxy同連線成對reorder有
合成persistent backend測試；目前runtime每個outbound連線一筆request，process情境中的
單筆reorder呈現有界delay，未宣稱跨連線強制交換。sim具真正事件重排。

替代驗收是有限工程證據。獨立代理審查與使用者的最後人工審閱也必須分開。
