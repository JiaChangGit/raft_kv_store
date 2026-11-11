# ai-general-framework 改進建議：本次實作的觀察與待驗假說

更新日期：2026-11-10。此文件是目標專案對相鄰 `ai-general-framework` 的**獨立建議**；
本次沒有修改框架。候選方案供後續討論與實驗，不是已批准的修改清單。只有取得針對
框架的新任務及相應授權後，才在該專案依自己的規則變更與驗收。

## 依據與可歸因的範圍

本次框架基準的 HEAD 曾記錄為 `2569a91156451b9138b3375bacd87672c64040ad`；
當時 148 個已盤點檔案的內容雜湊與工程強化完成時一致，見
`artifacts/reports/framework-source.json`、`artifacts/reports/environment.json`、
`artifacts/reports/engineering-stage2.json`。框架工作樹原本有未提交修改，HEAD
本身不等於完整內容版本；未來要記錄實際樹狀態和內容 hash。這些本機 artifacts
被 Git 忽略，複製來源時需確認證據仍可取得。

| 實際觀察 | 可支持的結論與限制 |
|---|---|
| 工作中明確讀取過 trace、debug、review、研究及教學指引；獨立審查找到多個可重現的目標專案缺陷 | 知道這些指引曾被讀取；沒有無 skill 對照，無法量化其對發現率或耗時的因果影響，也不能推定宿主自動觸發 |
| `agf.py run` 保存 release campaign 的命令、stdout/stderr、退出碼與完整性標記 | 這次有可檢查證據，見 `artifacts/runs/engineering-release-campaign/result.json`；尚未比較沒有 helper 時的返工與成本 |
| 目標專案 checker 起初會忽略 source symlink，接受不合型別的 report；修正和負向測試有保存 | 這是**目標專案驗收工具**的缺陷，不是已證明的框架錯誤；見 `artifacts/audit/oracle-gate-red.txt` 和 `docs/ENGINEERING_HARDENING.md` |
| follower 離線後 retry 風暴曾由真實三程序重現；後來測試可攔截 | 這是目標 runtime 與測試的觀察，不可直接歸責框架；首敗見 `artifacts/audit/retry-red.txt`，實際輸出見 `artifacts/audit/follower-down-retry/` |
| 新增交接文件改變目標專案的 source fingerprint，使先前 receipt 顯示 source changed | 這是目標專案目前 checker 的全來源策略，能避免舊證據被當成新證據；不表示框架有相同缺陷，也不能據此降低 gate |
| 切換至 Claude Code 時手動製作入口與交接；讀取過框架規則與 skill | 是否被 Claude 自動發現、是否漏觸／誤觸、跨模型延續效果與 token 用量皆未知 |

未知資訊維持未知：沒有本次完整的有／無 skill 成對資料，沒有可核對的總 token
與費用，亦沒有可比較的真人理解結果。Windows 跳過的 symlink 測試是目標專案
checker 在當時環境的結果，不應直接歸責框架的跨平台能力。

## 可考慮修改既有內容

1. **跨宿主交接與證據時效**：先查看框架 `docs/workflow.md` 的現有交接和記憶指引。
   它已要求核對版本、未提交變更、驗證時效與權限；先評估是否足夠。若兩個以上
   真實任務反覆漏掉「交接檔本身改變來源指紋」或「ignored artifacts 在新環境缺席」，
   可考慮在既有交接段落補一個短例子：列出證據位置、是否隨版本搬移，以及新增
   文件後 receipt 的適用版本。目的在降低接手時誤報；不可要求每個小任務多做文件，
   也不可讓說明取代專案驗收規則。
2. **審查有界資源與錯誤恢復**：`skills/review-change/SKILL.md` 已要求只報可證實的
   觸發、路徑與後果，也提及資源生命週期。可用真實案例檢查是否還需要在其按需
   reference 或現有文字補充「持續失敗的重試與飢餓」的通用審查提示；如果目前
   指引已能讓受測者穩定發現，就不加。不要加入 Raft、固定埠數或本專案具體常數。
3. **命令證據的成功與完整性**：`agf.py run` 本次已輸出 `capture_complete` 和
   `metadata_saved`。可先檢查 `docs/verification.md`／`docs/workflow.md` 是否已清楚
   說明「程式退出成功」與「證據已完整保存」各自的意義；若發現真實歧義，再以
   最小文字修正。不要先增加 wrapper 或把某個專案的驗收 schema 當通用標準。

## 可考慮新增、刪除或合併的條件

- **新增範例，不先新增 skill、script 或 MCP。** 若上述案例在兩個以上不同專案
  可重現，且既有指引不易使用，可在適當的既有 `evals/paired/` 或 references 增加
  經去識別的最小正反例。先比較宿主原生搜尋、現有 skill 和既有 helper；只有有
  明確能力缺口與效果證據，才評估新增共用工具或連線。
- **刪除／合併僅為候選動作，目前沒有足夠證據指定必刪檔案。** 維護者可先對照
  框架 `docs/workflow.md`、`docs/maintenance.md` 及各 skill 描述；若實際找到重複
  規則、重複觸發入口或不再引用的內容，逐項核對用途、引用與相近負例，保留最窄
  的權威位置，再提議刪除或合併。不要因本次工作較長就臆測某個 skill 多餘。
- **來源指紋的輔助說明只作假說。** 對能清楚檢查影響範圍的專案，或可在交接中
  同時顯示內容 hash、變更檔案及歷史 receipt 的適用範圍，幫助使用者判斷後續測試；
  不得因此自動宣布文件變更無影響，或讓既有 gate 接受不符指紋的結果。優先使用
  現有 Git／專案工具，未見可重現的共通瓶頸前不新增 script 或索引。

## 分類、正反例與對照驗證

先取得原始任務、版本、指令、stdout/stderr、事件與失敗重現；比較缺失究竟出在：
目標規格含糊、目標程式或測試、框架指引、skill 描述／引用、helper、宿主發現／
權限、模型選擇，或測試環境。單次對話的失敗只記為觀察；有機制解釋的是待驗假說；
可跨任務重現、具來源與適用範圍的結果才考慮升格為長期約定。

| 假說 | 真實正例 | 相近負例與副作用 |
|---|---|---|
| 簡短交接提示能避免把舊 receipt 說成目前通過 | 在隔離複本保留舊 receipt，加入文件，讓新 session 接手 | 給出未變來源及有效 receipt；不能無故宣稱過期、要求全量重驗或產生冗長流程 |
| 資源回復提示能協助審查 | 以去識別的有界 retry／餓死狀態案例，提供可驗證執行路徑 | 有正常 backoff、遲到回覆處理或僅為風格差異的相近案例；不能製造不可行的缺陷 |
| 指引能區分任務成功與證據完整 | 命令成功但 capture 被截斷或 metadata 保存失敗 | 完整輸出且退出成功；不能把正常成功誤報成失敗 |
| Claude 能發現所配置 skill | 在隔離專案以相同版本配置，查宿主實際載入紀錄 | 未配置、描述相近但不適用的任務；僅從回答推測載入屬未知 |

有／無 skill 對照參考框架既有 `evals/paired/README.md`：使用隔離且內容相同的
材料，固定 host、model、工具與權限，控制版本及執行順序；先定測試者持有的判準，
不要把答案或隱藏驗收放進被測輸入。分別記錄正確性、越權、漏觸／誤觸、重工、
必要互動、耗時與宿主提供的 token；缺值標示未知，cached input 是 input 子集，
不可重複加總。小樣本與不同模型不能直接推論長期改善。

## 如日後授權修改框架

1. 先讀該專案 `AGENTS.md`、`docs/maintenance.md`、既有規則及目前工作樹；保留
   原有未提交變更。重新確認這次建議與框架現況是否仍相符。
2. 在最窄的現有文件、skill、reference 或 helper 提出具體變更；由實例顯示問題及
   相近負例，必要時做有／無 skill 對照。以實際結果決定保留、修訂或放棄。
3. 對受影響行為執行適當測試；依框架規則執行
   `python -m unittest discover -s tests -v` 與 `python scripts/agf.py audit`。
   涉及 portable 行為時於 Windows 和 Linux／WSL 跑相關共同案例；不可將缺工具或
   skipped 記成通過。
4. 若修改 discovery、部署或更新，依 `docs/maintenance.md` 在隔離目標驗 shared/copy、
   相對路徑、drift、recovery 與實際宿主載入；格式 audit 無法替代宿主行為驗證。
5. 記錄框架版本與工作樹、測試輸入、命令、環境、結果、限制、可取得的耗時和 token，
   保留首敗。目標專案和框架各有自己的變更與驗收；不要將一方通過套用到另一方。
