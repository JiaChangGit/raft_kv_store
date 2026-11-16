# 框架實際使用與回饋

> **2026-11-16 起，這份文件由 [S8 的分析報告](../framework-study/analysis/report.md) 取代。** 這裡保留原文作為早期紀錄；仍有效的建議已併入新報告，之後只更新新報告。

ai-general-framework維持唯讀，HEAD識別與檔案hash在artifacts/reports/framework-source.json
及environment.json。目標配置sharedtools與read-research/trace-code/review-change/
debug-root-cause/guide-learning，AGENTS原樣保留並明確引用project-rules。

## 觀察

- 根代理與分工代理明確讀取規格/追蹤/審查skills；這支持「有讀取」而非宿主自動觸發。
- 獨立審查找到慢ACK淘汰、缺timer、proxyheldframe、oracle序號語意與時點漏洞；
  這些問題有具體程式路徑和修正/回歸，沒有只把模型同意當證明。
- 使用框架run --output-dir保存完整releasecampaign的stdout/stderr/result；
  capture_complete與metadata_saved均可核對。成功與證據保存分開判讀。
- 尚未比較無skill基準，不能把上述缺陷發現或73秒左右該次執行歸因於框架增益。
- 真實宿主自動探索率、漏觸率、token與成本未知；沒有新增MCP/索引或改框架。

## 問題分層

P05隔離與P06proxy的依賴屬目標roadmap；以最小測試設施前移並明示。
慢ACK/重啟query屬目標code/harness；strictseq模型屬目標checker，不直接歸框架。
只有重複出現、可跨專案重現的缺漏，才考慮在通用指引/skill/reference/helper最窄位置修改。
Raftterm/quorum/fsync規則留在本專案。

## 後續可驗假說

1. trace契約是否有助於區分plannedlast_applied與實際KVappliedindex？
2. review-change是否比相同條件無skill更早發現heartbeat與RPCtimeout界線？
3. run證據保存是否減少為補診斷而重跑？

各假說用相同版本/host/model/工具/材料/權限的新session，搭配相近負例與有無skill對照。
判準先看正確性、越界、返工與必要互動，再看可取得usage。快取token不能重複加總，
字數不能換算實際token。一次觀察先保留原文，不升格永久約定。

未來若另行修改框架，先依其AGENTS/maintenance執行受影響unittest與audit；portable
行為另驗Windows及Linux/WSL，部署變更驗shared/copy、drift/recovery與實際宿主載入。
這次只驗目標專案，不把框架歷史測試紀錄當本次結果。
