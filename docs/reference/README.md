# Distributed KV Store with Raft — Codex Implementation Kit

**Version:** 1.0 | **Review date:** 2026-11-10 | **Language:** Traditional Chinese (specification) / Rust (implementation)

這是一份**已完成可行性與架構審查**、可供 Codex CLI 逐階段實作的工程規格套件。它**不是**已完成的 Raft 原始碼，也**不表示**任何未來生成的程式已通過測試。

## 核心決策

- Rust stable + std::net TCP + std::fs + serde/tracing（不得依賴現成 Raft 實作）
- 3 個固定 Node、1 個 Raft Group、Quorum=2；3 個獨立 Linux Process
- Single-owner Raft event loop，其他 Thread 僅傳遞事件、送收 RPC
- Append-only WAL，`currentTerm`/`votedFor`/log 為 durable state；WAL ACK 僅於同步成功後發出
- V1 以 Replicated GET 提供線性化讀取，**不實作** ReadIndex/Lease Read
- 原 Client ID + Sequence 的重試使用 replicated session table，結果可由 committed log 重建
- 同一份 `raft-core` 供 deterministic simulator 與真實 TCP runtime 使用
- V1 無 Snapshot、Dynamic Membership、TLS、Sharding、Multi-Raft

## 下載後在 WSL2 Ubuntu 使用

```bash
# 建議在 WSL2 Linux 檔案系統操作（例如 ~/projects），不要在 /mnt/c 進行 WAL fsync 實驗
unzip raft_codex_implementation_kit.zip -d ~/projects
cd ~/projects/raft_codex_implementation_kit

# 先安裝 Rust stable + rustfmt + clippy、Python 3 + pytest、Codex CLI
bash scripts/preflight.sh

# 安裝、登入 Codex CLI，請參考官方文件（通常使用 `codex login`）
# 僅執行當前階段；必須通過 gate 才能開始下一階段
bash scripts/run_phase.sh 00
bash scripts/run_phase.sh 01
# ...依序至 08

# 單獨重新執行某階段 gate
python3 scripts/verify_gate.py 01
```

首次安裝方式與 Codex CLI 旗標可能隨版本變更，建議參考 https://learn.chatgpt.com/docs/cli/reference 。此套件使用已核對的 `codex exec --sandbox workspace-write --cd <dir> -` 形式。**不要使用 `--yolo` 或 `danger-full-access`。**

> `run_phase.sh` 使用 Codex CLI 依階段實作並在結束後執行本地 gate。它不會自行跨越 gate；若測試失敗，**不得**前進下一階段。需要先確認 Codex 已授權、依賴已準備完成。Codex 執行、實際編譯與測試必須在您的 WSL2 環境進行。

## 目錄

| 位置 | 內容 |
|---|---|
| `AGENTS.md` | Codex 的不可變工程守則與定義 |
| `docs/01_FEASIBILITY_REVIEW.md` | 可行性與架構審查、風險、Go/No-Go |
| `docs/02_ARCHITECTURE_RFC.md` | 模組架構、狀態圖、流程與 API 契約 |
| `docs/03_PROTOCOL_STORAGE.md` | RPC Protocol、持久格式、同步順序 |
| `docs/04_INVARIANTS_TEST_PLAN.md` | 不變條件、測試矩陣與 Test Oracle |
| `docs/05_RECOVERY_CLIENT_SEMANTICS.md` | Recovery、GET、Retry、Deduplication |
| `docs/06_ENVIRONMENT_OPERATIONS.md` | Windows 11 / WSL2、操作與故障注入 |
| `docs/07_IMPLEMENTATION_ROADMAP.md` | 依賴關係、預估工作量及 Gate |
| `docs/08_RAFT_PAPER_TRACEABILITY.md` | 論文來源與規則追蹤 |
| `docs/09_DECISIONS_ADR.md` | 設計決策記錄 |
| `docs/10_RELEASE_ACCEPTANCE.md` | 最終可交付驗收條件 |
| `codex/PHASE_00_...md` ~ `PHASE_08_...md` | 可直接交給 Codex 的逐階段規格 |
| `gates.json` | 自動化驗收門檻；含測試名稱與必要檔案 |
| `scripts/run_phase.sh` | 單階段執行器 |
| `scripts/verify_gate.py` | 本地命令驗證 + gate receipt |
| `scripts/preflight.sh` | 工具與環境檢查 |
| `diagrams/*.mmd` | Mermaid 原始圖 |

## 使用規範

1. 閱讀 `docs/01_FEASIBILITY_REVIEW.md` 的**阻擋項目**，不得用繞過方式消除風險。
2. Codex 嚴格遵守 `AGENTS.md`，只修改當前 Phase 允許的檔案。
3. Phase 指定測試名稱必須真正存在；`verify_gate.py` 會檢查 Test Discovery，防止空測試通過。
4. 若 Gate 不通過，修改**當前階段實作**直到通過；不可修改 Gate 定義來配合錯誤程式。
5. Git checkpoint 請於各 Phase 成功後自行建立，並保存 `.gates/`、`artifacts/` 結果供審查。
6. 自動驗收是工程證據，不是形式化定理證明；不能據此宣稱實作已被完全證明正確。

## 參考

- Raft Extended Paper: https://raft.github.io/raft.pdf
- Codex CLI: https://learn.chatgpt.com/docs/cli/reference
- Codex non-interactive: https://learn.chatgpt.com/docs/non-interactive-mode
