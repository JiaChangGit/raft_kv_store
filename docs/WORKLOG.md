# 實作工作紀錄

> **V1 時期的工作紀錄（2026-11-10 起），之後不再更新。** 2026-11-12 以後的經過寫在[路線圖](ROADMAP.md)、各階段的規格補記與[交接文件](HANDOFF_CLAUDE.md)。

## 2026-11-10：範圍與驗收基準

使用者確認在 `/home/user/src/Distributed_Key-Value_Store_with_Raft` 建立完整 V1，沒有額外原始套件，允許準備必要環境。`ai-general-framework` 維持唯讀；目標專案可配置其指引與 skills。未授權的 push、PR 與部署不在本次交付內。

三份原始 Markdown 原位保留；23 段內嵌來源抽成 `docs/reference/`。識別雜湊與原始資產缺口見 `SOURCE_MANIFEST.json`。替代契約已建立為 `acceptance/v1.json`；原始 gate 不可取得，所有結果都應稱替代驗收 v1。

本節建立時：所有 P00–P08 gate 尚未執行，沒有測試或 benchmark 通過聲明。後續每階段以實際 receipt 與原始輸出更新狀態，不能將本節規劃當作結果。

驗收工具先做限定檢查：Windows 原生 Python 執行 `python -B scripts/test_verify_gate.py -v`，四項 parser 正反案例通過、退出碼 0；案例涵蓋缺名、同名歧義、子字串誤配、零測試與 ignored 不得算成功。WSL 執行 `bash -n scripts/preflight.sh scripts/run_phase.sh`，退出碼 0。這些是 checker 自身的局部檢查，不代表 P00 或任何 Raft gate 通過，未驗證整個 checker 的所有 subprocess／檔案失敗路徑。

## 階段證據寫法

每次完成或受阻時，保存當時版本／source fingerprint、修改範圍與理由、讀取的來源、實際命令／cwd／工具版本、退出碼、執行／失敗／忽略數、原始輸出位置、首次失敗與修正、下一步及操作邊界。未知數字填未知，不估計 token 或費用。

接手者先核對目前檔案與未提交差異，確認舊結果仍適用。`recorded_at` 不代表剛重跑測試。一次觀察、待驗假說與已驗約定分開；Raft 專屬規則留在本專案。

## 同日後續實作與續接

七個Rustcrates、Pythonproxy/client/cluster/checker/benchmark、教學圖源均已建立。
環境為user rustup Rust1.99.1+rustfmt/clippy、專案venv pytest9.2.0；未使用sudo、未改全域PATH。
P00/P01有早期成功receipt；之後收斂修改checker以在pytest前建置真正bins，因此完整鏈須
以新的checkerhash從00重驗。早期結果保留，不當作新版本最終結果。

1000×10000releasecampaign、真process測試、benchmark和隔離mutation已有實際執行證據。
首敗、修正與限制見VERIFICATION_FINDINGS，不把重試成功當根因證明。
初始Windows/WSLcheckerparser為局部驗證；目標runtime交付環境為WSL/Linux，不宣稱Windowsruntime。

接手第一步執行 `.venv/bin/python scripts/status.py`，讀latestattempt與原始logs，再核對現況。
若有FAIL/NOT_RUN，先處理第一個未滿足階段；全部PASS後的下一步是使用者人工審閱或另行指定
的新工作，而非自動push/PR/deploy。來源文字/hash與證據不可被摘要取代。

## 實際操作與教學驗收

使用者決定三個後續方向皆做，但明確要求**先只完成實際操作與教學驗收，完成即停止**，
由使用者切換模型後再處理工程強化，最後才討論 V2。本次完成後必須停止，等待切換模型後續接。

已在隔離目錄實際啟動三個 process，使用 client CLI 執行 PUT／GET，SIGKILL 當時的
Leader、觀察其他節點重新選舉、執行新的 GET，再以原資料目錄重啟舊節點並觀察追趕。
兩次實測結果、命令與來源 hash 在 `artifacts/runs/hands-on-20261110-01/result.json` 和
`artifacts/runs/hands-on-20261110-02/result.json`；
手動操作、歷史結果與 trace 邊界見 [HANDS_ON.md](HANDS_ON.md)。人工理解尚待使用者練習。
README 的固定埠直連設定也已用實際 client、SIGKILL 與重啟另行驗證，紀錄在
`artifacts/runs/manual-direct-check/result.json`；這與前述經 proxy 的示範是兩種不同路徑。

兩張圖源已用專案內隔離 Mermaid CLI 12.2.0 與本機 Windows Node／Chrome 渲染 SVG，
並以 2x PNG 作視覺檢查。重現腳本與該次工具位於 `.agf/qa/mermaid-render/`，本機路徑
與軟體版本可能變動；SVG 渲染成功不證明所有圖意。`ai-general-framework` 仍為唯讀。

下一位模型先核對目前 source fingerprint 與最新 P08 receipt，區分這次教學檔案變更與
先前 V1 執行證據，然後依使用者指示從工程強化開始。不要預設 V2 的功能或自行 push／部署。

## 工程強化續接

使用者以「繼續」啟動工程強化。已對目前程式作獨立審查並保留具體重現，修正範圍
包括 WAL 開啟／回復 barrier、proxy partial-frame、history/report/source snapshot 驗證、
runtime worker 滿額與 I/O 總 deadline。方法、風險與格式限制見
[ENGINEERING_HARDENING.md](ENGINEERING_HARDENING.md)。

驗證結果以 `artifacts/reports/engineering-stage2.json`、乾淨副本的 result.json 及最新
P00～P08 receipt 為準；文件列出方法不等於已跑完。checker 變更需從 P00 重驗，不能
挪用上一輪不同 checker hash 的成功結果。相鄰框架保持唯讀。

工程強化完成後停下。下一階段依先前約定先做 V2 範圍與驗收設計；WAL header integrity
是已有反例的候選，snapshot/compaction 等仍須明確取捨，不能直接實作或擴張 AGENTS 規則。

