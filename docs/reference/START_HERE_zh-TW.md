# 從這裡開始：Raft KV Store 工程審查與 Codex 分階段實作套件

## 已交付 vs 尚未執行

**已交付**：可行性與架構審查（Conditional GO）、完整架構與 API 契約、持久化與回復規格、Raft 安全性不變條件、九個階段 Codex 可執行任務、具名 Gate、自動執行器、Mermaid 圖、驗收/追蹤文件。

**尚未執行**：正式 Raft 程式碼生成、Codex CLI 在您 WSL2 的實際執行、Rust 編譯、真實 Process Fault 測試、實際 benchmark；不得將下載此 ZIP 解讀為這些工作已完成。

## 核心審查結論

- 可行：Rust stable + 三獨立 Process + 三固定節點 + 一 Raft group，在 WSL2 Ubuntu 與 12 GB RAM 下有合理資源餘裕。
- 不可跳過的 P0：term/vote/log Durable ACK、Current-Term Commit、Recovery 不套用未 Commit WAL、Replicated GET、Client Deduplication、Simulator 重用正式 Core。
- 優先順序：P00 scaffold/sim → P01 election → P02 replication+commit → P03 WAL+recovery → P04 TCP runtime → P05 KV + linearizable GET → P06 fault injection → P07 independent checker → P08 benchmark+release evidence。

## 在 WSL2 內執行

先以 `https://learn.chatgpt.com/docs/cli` 安裝/登入 Codex，並安裝 Rust stable / rustfmt / clippy / pytest。之後：

```bash
unzip raft_codex_implementation_kit.zip -d ~/projects
cd ~/projects/raft_codex_implementation_kit
bash scripts/preflight.sh
bash scripts/run_phase.sh 00
# P00 通過 Gate、人工審查後
bash scripts/run_phase.sh 01
# ...繼續至 08
```

每個 Phase 的任務規格位於 `codex/PHASE_XX_*.md`。腳本會將該檔案傳給 Codex CLI，使用 `workspace-write` sandbox，記錄輸出，再執行 `scripts/verify_gate.py`。任何 Gate 失敗都應停止，不能直接跳下一階段。

## 最重要的文件

1. `docs/01_FEASIBILITY_REVIEW.md` — 條件式 Go 與 14 項架構審查。
2. `docs/02_ARCHITECTURE_RFC.md` — 模組與事件/Effect/Durability 契約。
3. `docs/03_PROTOCOL_STORAGE.md` — RPC、WAL Format、fsync barriers。
4. `docs/04_INVARIANTS_TEST_PLAN.md` — Figure 3 Safety 與 26 個故障情境。
5. `AGENTS.md` — Codex 必須遵守的約束。
6. `gates.json` — 具名自動驗收基準。
7. `codex/HOW_TO_RUN.md` — Codex CLI 操作與門檻。

最後請閱讀 `docs/10_RELEASE_ACCEPTANCE.md`，理解什麼情況才可以宣稱專案完成。
