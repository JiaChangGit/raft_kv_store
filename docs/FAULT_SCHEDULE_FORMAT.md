# 故障排程與重現

> **V1 時期撰寫。** 故障排程的格式與重現方式仍然適用；S4 之後 `tools.cluster.Cluster` 可以啟動 1～7 台（預設 3 台），route 與 PID 的數量隨節點數而定。

`tools.cluster.Cluster` 為每次實驗產生六條方向性 route、三個獨立 PID 與原資料目錄。
faults.jsonl 記錄 sequence、monotonic time、rule/source/target/mode 及每個 frame 的 rpc_id。
模式 allow/drop/delay/duplicate/reorder 作用於完整 frame；reorder 成對交換，單筆超時 flush。
目前正式 runtime 一個連線一個 request，因此同連線成對 reorder 以合成 persistent backend 測試，
process 測試中的單筆 reorder 是有界延遲；不宣稱已測跨連線強制交換。sim 支援真正事件重排。
規則在傳送延遲後再查一次，partition 同時作用於既有與新連線；route 配置禁止 fallback。

測試重現：`.venv/bin/python -m pytest -q tests/integration/test_faults.py`。
真實 process thread/network schedule 不是完全 deterministic；保存實際 schedule 與 history，
重跑相同 seed 只能重建輸入計畫，不能保證 OS 排程逐事件相同。精確事件 replay 使用 raft-sim。
SIGKILL 與 graceful shutdown 分開；測試使用 SIGKILL 和原資料目錄 restart。
穩定網路、多數健康、磁碟可用時才評估 bounded progress；timeout 不等於 safety violation。
