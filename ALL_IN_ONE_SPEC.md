# Distributed Key-Value Store with Raft — Complete Reviewed Specification

> 此彙整版方便搜尋與閱讀；正式執行以各原始分檔及 gates.json 為準。


---

<!-- Source: START_HERE_zh-TW.md -->

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


---

<!-- Source: README.md -->

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


---

<!-- Source: AGENTS.md -->

# AGENTS.md — Codex Mandatory Project Rules

You are implementing a **3-process, fixed-membership Rust Raft replicated KV store** in this repository. Treat documents as binding engineering requirements, especially the Raft paper's Figure 2/Figure 3 and §5.4.

## Non-negotiable constraints

1. Implement Raft consensus **from scratch**; never add `openraft`, `raft-rs`, `tikv/raft`, etc.
2. Fixed node IDs 1,2,3; quorum 2; one cluster / one Raft group. No snapshots/compaction, membership changes, sharding, TLS, Web UI, external consensus systems or Kubernetes.
3. One actual `raft-core` is used in simulation and Linux runtime; **do not duplicate consensus logic**.
4. Own consensus state inside one event-loop thread. Transport/Timer/Storage may emit messages/effects but must not directly mutate Raft state.
5. Persist `term`, `vote`, and log mutations before sending RPC responses that depend on them. WAL durable failures fail closed; never silently continue.
6. Current-term commit restriction: majority replication alone DOES NOT directly commit older-term entries. `log[N].term == currentTerm` must hold when leader advances commitIndex to N.
7. Leader's own matchIndex counts **only durable local entries**. Follower ACK counts **only verified durable matching prefix**. Ignore stale RPC replies or replies from old terms/leadership epochs.
8. Restart always starts Follower with volatile `commit_index=0`, `last_applied=0`; only apply provably committed entries, never replay the entire WAL directly into KV.
9. V1 GET is a replicated log command (not local read; not ReadIndex). All successful reads/writes require a safe committed/apply path.
10. Client idempotency: client ID + monotonic sequence, one outstanding logical request per client; durable-by-replay session results; a duplicate must not execute the command twice. Never answer successful requests from a deposed or isolated leader's cache.
11. Avoid `unsafe` Rust unless a narrow, documented and reviewed necessity appears (should not in V1).
12. All transport paths under fault-test go through directional proxies. A TCP stream may fragment bytes; only full frames may be delayed/dropped/duplicated/reordered in the simulated proxy.
13. Tests are required, especially negative safety tests. Do not mark gates passed by creating empty/stub tests or weakening assertions.
14. Maintain stable interfaces, concise comments at subtle invariants, structured logging and test reproducibility with seeds.
15. Only work on the phase specified by the caller. Never skip ahead or make phase scope expansions without a documented blocking dependency.
16. Respect fixed bounds: key <=256 bytes, value <=4096 bytes, frame <=1MiB; reject malformed inputs before allocating unbounded memory.

## Required output from each phase

- Deliver implementation and tests for that phase; format + unit/integration tests where specified.
- Explain invariants touched, code files modified, tests run, actual pass/fail, remaining risks and deviations.
- Do not claim passing results without showing the commands and exit codes.
- Do not edit `gates.json`, `scripts/verify_gate.py`, `scripts/run_phase.sh`, `AGENTS.md` to bypass requirements.
- Use repo-relative paths; no writes outside workspace unless explicitly authorized.
- Do not automatically run later phases; wait for the independent gate check.

## Engineering conventions

- Cargo workspace packages: `raft-core`, `raft-protocol`, `raft-storage`, `kv-state`, `raft-sim`, `raft-transport`, `raft-node` as the phases introduce them.
- Primary Rust tests are `#[test]` or appropriate supported async test and named exactly as in `gates.json`. No invented binary output.
- Python integration uses `python3 -m pytest` and the filenames referenced in `gates.json`.
- No liveness claim for permanently unstable network; safety is assessed separately from liveness.
- Use Linux monotonic timers and separate stdout/stderr for each node.

Read `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the current `codex/PHASE_XX_*.md` before editing code.


---

<!-- Source: docs/01_FEASIBILITY_REVIEW.md -->

# 01. 可行性與架構審查（Architecture Review）

**審查結果：CONDITIONAL GO**。範圍可執行，但下列 P0 風險必須在對應 Gate 前被實作及測試證據消除。這是**設計審查**，不是對尚未生成程式的測試通過聲明。

## 1. 實現可行性

| 維度 | 結論 | 原因 / 約束 |
|---|---|---|
| Rust stable | GO | 能降低記憶體安全與 Thread ownership 風險，無法取代 Raft logical correctness |
| Windows 11 + WSL2 Ubuntu | GO | 在 Linux home/ext4 中執行三個 Process 與 WAL，不在 `/mnt/c` 量測 Durability |
| 12 GB RAM | GO | 三個小型 Rust process、Python proxy、pytest、有限 WAL；無 snapshot，因此測試資料需控制 |
| 3 Nodes、1 Raft Group | GO | 3 節點 Quorum=2；支援 1 node 故障，不能同時承受 2 nodes 故障而繼續提交 |
| TCP + multithreading | GO with constraints | 必須使用 single-owner event loop、framed messages、single in-flight AppendEntries/peer |
| WAL persistence | GO with constraints | 同步 WAL transaction + CRC + fsync；主機斷電/壞碟不在完整保證範圍 |
| Linearizable read | GO with design change | 先採「GET 寫入 Raft Log」，避免 V1 自製不正確的 lease/read index |
| Random fault simulation | GO | 虛擬時間 + 可重現 seed + **正式 raft-core 重用**；不可另造簡版演算法 |
| Codex phased execution | GO with human gates | Codex 可以按 spec 改檔並跑測試；必須逐階段審核，不能以自行宣稱代替驗收 |

## 2. P0 Blocking Findings — 必修正

### AR-01：Persistence ordering / RPC response barrier — P0

風險：Raft 邏輯先更新 RAM、立刻回 RPC Success，WAL 在稍後才落盤。若 Crash，Leader 會錯誤計入一個不存在的 Durable ACK。

**決議**：`step(Event)` 可規劃有序 effects，但 runtime 必須在 `Persist` 同步成功後才執行相依的 Send/Reply/Apply/Local-Durable-Match effects。V1 用單一 actor + 同步 fsync 減少 Pending I/O 狀態；失敗後停止不安全的操作，Fail Closed。

**Gate**：P03，`append_ack_after_durable_sync` 與真實 crash failpoints。

### AR-02：Current-term commit restriction — P0

風險：看到舊 Term Entry 在過半數節點上就直接將其視為 committed。

**決議**：只有 `N > commitIndex`、多數 `matchIndex >= N`、且 `log[N].term == currentTerm` 才能將 Leader `commitIndex` 提升至 N。之前的 prefix 因當前 Term Entry Commit 而間接 Commit。

**Gate**：P02，`previous_term_entry_requires_current_term_commit`，並以 paper Figure 8 推導說明。

### AR-03：Recovery cannot apply all WAL entries — P0

風險：WAL 裡的 Log 可能還沒 Commit，Restart 後直接 replay 為 KV 會造成外部可見資料錯誤。

**決議**：重啟時回復 persistent term/vote/log；`role=Follower`、`commitIndex=lastApplied=0`，KV/Session 從已證實 committed 的 prefix 依序 apply；新 Leader 先 Append 並 Commit current-term NoOp。Follower 由安全的 `leaderCommit` 通知推進，不能 apply 不匹配的 suffix。

**Gate**：P03，`uncommitted_suffix_not_applied_after_recovery`。

### AR-04：Linearizable read on isolated old leader — P0

風險：舊 Leader 在 Partition 後仍有舊 KV Map，直接 local GET 造成 Stale Read。

**決議**：V1 所有新 GET 均作為 Replicated Log Entry，在 Commit + Apply 後回結果；ReadIndex/Lease 不納入 V1。對重試/快取也不可在缺乏安全前提時直接由舊 Leader 回成功。

**Gate**：P05、P07，`replicated_get_linearizable` 與 partition tests。

### AR-05：Client retry exactly-once effects — P0

風險：Client Timeout 發生在 Entry Commit 後但回覆前，新 Leader 收到重試可能造成重複應用。

**決議**：透過 replicated Session Table 保存 `(client_id, seq, canonical_command, result)`，由 committed log 重建；單一 Client 只允許一個 in-flight logical request，重試必須保持同一 Sequence。相同 ID 不同內容明確拒絕，不能只依賴 Leader RAM Cache。

**Gate**：P05、P07，`retry_same_request_has_single_effect`、`same_id_different_command_rejected`、`test_retry_after_leader_crash`。

### AR-06：Simulation must match production core — P0

風險：寫兩套 Raft（Simulation 與 Production），模擬通過也無法說明生產邏輯正確。

**決議**：同一 `raft-core` 的 `step(Event)` / effect contract 注入 Virtual Clock / Fake Disk / Fake Network 或真實 runtime；所有不變條件由獨立 checker 監測。

**Gate**：P00、P07，`simulation_seed_is_replayable`，random history run。

## 3. P1 Non-blocking, but must have acceptance coverage

| ID | 風險 | 對策 |
|---|---|---|
| AR-07 | AppendEntries 回應延遲造成 `matchIndex` 假性進展 | 每 Peer 限制 1 in-flight；rpc_id + term/epoch 驗證；回覆只確認本次匹配 prefix |
| AR-08 | Follower 收到 heartbeat 後用自己的 last index apply 衝突 suffix | Commit 範圍最多到本次被確認匹配的 prefix end |
| AR-09 | 某 RPC 在升 Term 時 reset timer 過度，造成 liveness failure | 只在接收有效 leader AE / grant vote 等指定事件重置 election timeout |
| AR-10 | 錯誤的 WAL 尾部修復破壞 log safety | 只接受可證明末尾不完整 record；中間 CRC/error Fail Closed |
| AR-11 | Python Fault Proxy 誤對 TCP byte stream reorder | 僅對解析完成的 RPC frame 故障注入；測試不得繞過 Proxy |
| AR-12 | Gate `cargo test` 有 0 tests 仍 exit 0 | 先 Test Discovery 實際檢查 gate 名稱，再執行完整 workspace tests |
| AR-13 | WSL `/mnt/c` fsync 與 Linux 原生行為差異 | WAL / benchmark 位於 WSL2 ext4，公開平台限制 |
| AR-14 | No snapshot 下 WAL 永遠增長 | 明確測試資料限制、磁碟空間保護、排除長期資料庫運維用途 |

## 4. 假設與非保證

- 非 Byzantine：Process 可 stop/crash/restart，網路可 delay/drop/duplicate/reorder frames/partition；節點 ID / config 不被惡意偽造。
- 沒有 Dynamic Membership，固定 3 個資料目錄；**不是** 2 節點也能保證讀寫的 Active-Active Database。
- 安全性不依賴固定 clock speed；Liveness 需要最終穩定網路、多數節點正常、fsync 可完成及足夠時間。
- 認定 OS/SSD 正確實現成功返回的 `sync_all` durability；不承諾磁碟毀損、任意 power-loss、惡意資料篡改。
- V1 無 snapshot/compaction，測試規模應有限且執行前檢查可用磁碟空間。
- JSON RPC 為便於 debug 的選擇；不是高性能 production wire spec。

## 5. 刻意修正先前工程提案的模糊處

1. **Follower 取得 heartbeat 並不等於整份 Log 都被 Leader 驗證**；只能使用 `prevLogIndex` 加本次 AppendEntries 的連續 entries 作為已匹配範圍。
2. **Un-fsynced WAL record 可能在 Crash 後意外完整存在**，不能假設未 fsync 必然消失；保證的是「已 ACK 的必須 durable」，不是「未 ACK 的必定丟失」。
3. **單 Thread actor 不等於自動達成 durability**；effect ordering 必須具體有阻擋相依 Response 的 barrier。
4. **Client duplicate cache 不能取代 Quorum 確認**；尤其不得把過時 Leader 的 cache 回覆當作新的 GET 成功。
5. **單次測試成功不是 Safety Proof**。Formal proof 不在 V1，但精確反例與 invariant checker 是必交成果。

## 6. 審查決策

**CONDITIONAL GO → Engineering GO 的條件：** P00 Contracts 定義清楚；P01 Election 有負向測試；P02 current-term commit/collision tests；P03 durability/failpoint tests；P05 Linearizable GET 與重試；P06/P07 真實 process、partition 與 checker 過關。

**禁止偷跑：** 前一 Gate 未通過，不可跳到下一階段或以「差不多能跑」作為完成標準。


---

<!-- Source: docs/02_ARCHITECTURE_RFC.md -->

# 02. Architecture RFC — Fixed 3-Node Raft KV

## 1. 部署（唯一正式拓撲）

```
Clients (Python) --> Node RPC endpoint (7201/7202/7203)
                                   |
          +------------------------+-------------------------+
          |                        |                         |
       Node 1                   Node 2                    Node 3
   raft TCP 7101             raft TCP 7102              raft TCP 7103
   Raft actor                Raft actor                 Raft actor
   KV applied state          KV applied state           KV applied state
   WAL data/node-1           WAL data/node-2            WAL data/node-3
          |                        |                         |
          +-- directional framed RPC fault proxies ----------+
```

三個 `raft-node` 是不同 Linux Process。Config 固定 cluster ID、node IDs、peer address、client address、WAL directory；每 Node 的 data directory 不得交換。**測試環境**的 peer-addr 為 Python proxy endpoint，以利注入故障；非測試環境可直連 TCP。

## 2. Cargo Workspace 與責任

| Package | 責任 / 不可做 |
|---|---|
| `raft-protocol` | `NodeId`, `Term`, `Index`, `Command`, RequestVote, AppendEntries, Client envelope, encoding 與 bounds；不得內嵌 Raft algorithm |
| `raft-core` | 完整 Election、Log replication、Commit predicate、Event→Effects；不直接做 socket/fs/cargo runtime |
| `raft-sim` | Mock durable storage, virtual time, deterministic queue, independent oracle；絕不 fork consensus algorithm |
| `raft-storage` | durable transaction WAL/CRC/fsync/recovery；不決定 commit |
| `kv-state` | deterministic Put/Delete/Get/NoOp + SessionTable + result；絕不修改 Raft commit |
| `raft-transport` | TCP framing, reconnection, rpc_id, queue, request timeout；不修改 term/vote/log |
| `raft-node` | Single-owner Raft runtime、WAL effect barrier、CLI/config、endpoint、node process |
| `tests/integration` | Python subprocess + framed fault proxies + history/checkers |

## 3. Raft Core 契約

```
step(event: Event) -> OrderedEffects

Event (examples)
  ElectionTimeout{generation}
  HeartbeatTick{generation}
  RequestVoteReceived{peer, request, rpc_id}
  RequestVoteResponse{peer, response, rpc_id}
  AppendEntriesReceived{peer, request, rpc_id}
  AppendEntriesResponse{peer, response, rpc_id}
  ClientCommandReceived{request, connection_handle}
  StorageFailure{error}     [runtime fatal]

OrderedEffects (examples)
  PersistTxn{transaction, barrier_id}
  SendRpc{peer, frame, depends_on_barrier?}
  ReplyRpc{peer, rpc_id, result, depends_on_barrier?}
  ApplyUpTo{index, depends_on_barrier?}
  ReplyClient{token, result, depends_on_commit_apply}
  ScheduleTimer{kind,generation,duration}
  EmitTrace{fields}
```

可以在 Rust 中使用 enum / structs 定義；不要讓 Core 呼叫 OS 時鐘或阻塞 Socket。同步 V1 runtime 的最簡單規則：`PersistTxn` 順序執行並完成 `sync_all()`，只有成功後才執行依賴該交易的 Send/Reply/Apply；同步失敗直接停止不安全操作，不處理新 Consensus Events。不得讓 in-memory mutation 與未完成 durable mutation 在下一事件中混淆。

**注意**：從 `step` 返回後的 `RaftState` 可能暫時包含未 durable 的變更；V1 用 *synchronous barrier before processing next event* 確保其不被其他事件看見。由 runtime 觀測的 local match 必須代表 durable；不可讓 Commit Logic 把待 fsync 的 Entry 當作已持久保存。

## 4. Role 與 Leader 變更

- Restart ⇒ Follower。Follower 超時後增加 Term、自投票，先持久化，再發送 RequestVote。
- Candidate 得到 2 票成為 Leader，初始化 peer nextIndex、matchIndex，append current-term NoOp，持久化後複製。
- 所有 Node 收到較高 term，持久化新 term 與 `votedFor=None` 後按規則退回 Follower；僅 current term 的 responses 可被當作有效證據。
- 同 term 有效 AppendEntries 可使 Candidate 退回 Follower；stale-term message 不得 reset timer。
- Election timeout 隨機 400–800 ms；Heartbeat 100 ms；所有 timer 含 generation 以忽略過期 tick。只要有有效 leader AE 或給予票才重設 Follower election timer。

## 5. Log Invariants 與索引

- Index 0 是虛擬 sentinel (`term=0`)，不落 WAL；真實 entries 從 index 1。
- Log 無 gaps。`nextIndex` 是下一個要發送之 index；`matchIndex` 是經確認匹配且 durable 的最高 index。
- 一個 Peer 最多一個 outstanding AppendEntries，metadata 包含 `rpc_id, term, leader_epoch, prevLogIndex, entry_count`。
- Success 只能確認 `prevLogIndex + entries.len()`（含空 heartbeat 確認的 prev index），不能信任 peer 的 lastLogIndex 直接推高 matchIndex。
- Failure 僅對當前合法 Request 調整 nextIndex，不容許 stale failures 倒退已驗證 Prefix。
- 只有與新 entries 相同 index、不同 term 的衝突才 truncate 自該 index 起的 suffix；相同 term 的 entries 不覆寫。
- Follower `commitIndex = min(leaderCommit, confirmed_matched_prefix_end)`，不得假設 follower 多出未驗證 suffix 就可 apply。

## 6. Leader Commit

```
for N descending from locally_durable_last_index to commitIndex+1:
    if log[N].term == currentTerm and durable_match_count(N) >= 2:
        commitIndex = N
        break
```

`durable_match_count` 計 Leader 自己（僅 fsync 成功後）及至少一個 Follower。當前 Term Entry 提交後，較早合法 prefix **間接 committed**。已提交 log 不刪改；Apply 遵守 `lastApplied <= commitIndex` 及 index 順序。

## 7. GET 與 Client

- V1 `Get` 也是 Command log entry，每個請求在 log 的線性化點是其 Commit；結果由 Apply 時計算。
- 所有成功回覆發生在本地 Apply 後。重試相同 request id 的舊結果只透過安全的 committed-apply path 回覆，不能只靠過時 Leader RAM Session。
- Client 在非 Leader 得到 `NOT_LEADER(leader_hint?)`；不可保證 hint 永遠正確。
- 超時必須標成 `UNKNOWN_OUTCOME`。Client 使用同一 `(client_id,sequence)` 重試，不生成新的 sequence。

## 8. State、Election、Dataflow、Recovery 圖

- `diagrams/raft_states.mmd`
- `diagrams/election.mmd`
- `diagrams/replication.mmd`
- `diagrams/recovery.mmd`
- `diagrams/modules.mmd`

這些 Mermaid 原始碼可以直接在 Markdown Preview / Mermaid Live Editor 查看，不影響可執行測試。


---

<!-- Source: docs/03_PROTOCOL_STORAGE.md -->

# 03. RPC Protocol & Persistence Format v1

## 1. Wire Protocol

每個 TCP frame 為 `u32_be payload_length` + UTF-8 JSON message；最大 Payload **1 MiB**，收到超限長度必須先拒絕、不得配置相同大小 buffer。Receiver 支援 TCP partial reads/writes；單一 stream 永遠以 frame 解析，proxy 故障注入僅操作完整 frame。

Envelope example（欄位定義，不等於必須逐字格式）：

```json
{"version":1,"cluster_id":"test-raft-1","from":1,"to":2,"rpc_id":42,"kind":"AppendEntries","body":{}}
```

- `cluster_id` 與 node 身分和 target 不符：拒絕，不能被視為合法 vote/ack。
- `rpc_id` 只用於 transport correlation；不能代替 Term 或 log-index 驗證。
- `version` 不符返回 protocol error；未知欄位策略固定且測試涵蓋。
- JSON payload 做 bounded deserialization、bounded strings/array；Key<=256 bytes、Value<=4096 bytes；Entries per AE<=32 初始值。

## 2. Raft RPC

### RequestVote

`Request{term:u64,candidate_id:u8,last_log_index:u64,last_log_term:u64}`

`Response{term:u64,vote_granted:bool}`

拒絕條件：request term 小於 currentTerm、已投給其他 candidate、或 Candidate Log 不夠新。比較 Log 新舊時先比 last_log_term，再比 last_log_index。對更高 term 必須同步持久化新 term、清空投票後才送出 response。投票成功時先 fsync 候選 node id。

### AppendEntries

`Request{term:u64,leader_id:u8,prev_log_index:u64,prev_log_term:u64,entries:[LogEntry],leader_commit:u64}`

`Response{term:u64,success:bool,matched_through?:u64,conflict_index?:u64,conflict_term?:u64}`

先檢查 term，`prevLogIndex/Term`；若衝突，針對本次新 Entries 的 suffix 進行 atomic truncate+append transaction。沒有修改也可回覆 Success，但 `matched_through` 必須是本次 request 實際驗證的最高 Prefix，不能報本地未知之 suffix。空 heartbeat success 可以確認 prevLogIndex；Follower 不得 Apply 不匹配 suffix。

**期初**可以只遞減 nextIndex 重試，`conflictTerm/conflictIndex` 為可選後續優化；不改安全性。

## 3. Client RPC

```
ClientRequest {version, client_id:u128, sequence:u64,
               operation: Put(key,value) | Delete(key) | Get(key)}
ClientResponse{request_id, status: OK|NOT_LEADER|TIMEOUT_UNKNOWN|
                     REQUEST_ID_REUSE|INVALID_ARGUMENT|STORAGE_ERROR,
               result?, leader_hint?, committed_index?}
```

NoOp 只能由 Raft Leader 產生、不能由 Client 注入。對同一 `(client_id,sequence)` 的 Command 內容必須一致。V1 同 Client 一次只有一個 logical outstanding operation；跨 leader 新請求靠 canonical command equality + deterministic session table。

## 4. Durable WAL Transaction Format

V1 binary record **建議**明確定義以下欄位（可在 P03 固定 offset，落實 round-trip tests）：

```
record = magic[4] | version:u16_le | record_type:u16_le |
         payload_len:u32_le | sequence:u64_le | payload:bytes |
         crc32c:u32_le
```

- `magic` 例如 `RFT1`；`version=1`；CRC32C 覆蓋 header + payload，不包含 checksum 自己。
- Record 必須以 deterministic encoding 定義 `HardStateUpdate{term,voted_for}`、`LogTxn{truncate_from?:index,append:[LogEntry]}`；允許 `Txn{hard_state?,truncate_from?,append?}` 一次表達多項變更。
- 在單一 record 同時含 truncate + append，避免重啟看到一半套用的奇怪中間狀態。
- 所有 record append 至 WAL，成功 `File::sync_all()` 才可對依賴者回 success；每次 fsync 不代表只能寫一個 entry，可 batch 但 V1 建議同步簡單優先。
- 初始檔案建立/rename 如採用原子更新 metadata，需同步父目錄 metadata；絕不假設有檔案卻必然已 durable。
- `node.meta` 固定包含 cluster_id,node_id,format_version；若 identity 衝突 Fail Closed。

### WAL Crash Recovery

1. 從頭掃描 Record：檢查 Magic、Version、長度上限、Sequence Monotonicity 與 CRC。
2. 完整有效的交易按順序 Replay 到 term/vote/log（**不是 KV**）。
3. **只在可確定的 incomplete trailing record** 容許截斷恢復；中間損壞 / 完整長度卻 CRC 錯誤一律 Fail Closed。
4. WAL 可以有未經 fsync 卻在 crash 後完整保留下來的交易；這是允許的，只要已 ACK 交易絕不遺失。
5. 重建 role=Follower、commitIndex=0、lastApplied=0、KV/Session empty；藉由後續合法 consensus knowledge 重新 Apply 已確認提交 prefix。
6. 在 Recovery 後重新啟動 IO / Timer；若 storage IO error 應保持不可用直到人工介入。

## 5. Durability / Visibility Contract

| Action | 必要 barrier |
|---|---|
| RequestVote grant | 更新 term/voted_for 已成功 fsync |
| Candidate 發出 RequestVote | 更新 term、自投票已成功 fsync |
| AppendEntries success with mutation | truncate/append 已 fsync |
| Leader 本地將自己列入 durable match | entry 已 fsync |
| 新 Term 之 stepdown response | 更高 term 已 fsync |
| Client success | entry 在合法 Commit Index 中，且本機 state machine 已 Apply |
| Apply | Index 已確認 Commit；不可直接依 WAL length Apply |

## 6. Explicit failures

- `fsync` / write failure：不可回 durable success，關閉不安全服務、發 trace，test 驗證。
- Malformed record：fail closed，不能跳過造成 index hole。
- Partial TCP send：繼續傳剩餘 bytes 或中止連線；不能把半個 frame 當成完整成功 RPC。
- Disk space exhaustion：停止接受新寫入並報錯，不能用 RAM-only commit fallback。

## 7. 需留待 P03 固定的細節

明確列出 Byte Offset、CRC32C Polynomial 及 Test Vector、截斷尾部原子性、`node.meta` 建立同步步驟、最大 WAL Record 長度。任何変更要更新文件、測試與版本，不能無聲改格式。


---

<!-- Source: docs/04_INVARIANTS_TEST_PLAN.md -->

# 04. Design Invariants & Verification Plan

## Raft Safety Invariants (paper Figure 3)

I1 **Election Safety**：同 term 最多一個合法 Leader。（只記錄已獲得多數投票者；不可依角色字串的時間切片混淆）。

I2 **Leader Append-Only**：Leader 只在自身 Log 尾端 append，不覆蓋/刪除自己的 Log。

I3 **Log Matching**：若兩個 Node 在同 index 有相同 term，則從 1..index 的 Log prefix 相同。

I4 **Leader Completeness**：已 Commit 的 Entry 一定出現在所有後續合法 Term Leader 的 Log 中。

I5 **State Machine Safety**：任何 Node 於 index i Apply Entry e，所有其他 Node 於 index i Apply 時只能 Apply 同 e。

## Engineering Invariants

- `currentTerm` durable monotonic across restart；同 term 同 Node at most one `votedFor`，允許重覆投給**同**候選人。
- 不存在 `lastApplied > commitIndex`；`lastApplied` 連續增加。
- 持久化 ACK 不能在 fsync 前發出；Leader self-match 只含 durable entries。
- Leader commit N 必須 current-term entry + quorum durable match；commitIndex 不倒退。
- Follower AE Success 只能確認受此次 RPC 驗證的匹配 prefix。
- `log[0]` 是虛擬 Sentinel，index 從 1，沒有 gaps。
- 不向 Client 回覆未 Commit/Apply 的成功；Unknown Outcome 不宣稱失敗。
- 同 `(client_id,seq)` 不執行多次 side effect，且結果可被 committed log replay 重現。
- Read 成功一定有 consensus 線性化點；partitioned old leader 不能本地直接成功回答。

## 測試分層（獨立 oracle）

1. Unit：Term/Vote/Conflict/Commit Predicate，包含 negative tests。
2. Deterministic Virtual Event Simulation：虛擬時計、訊息分割、drop、delay、duplicate、crash、recovery、seed replay。
3. Durable WAL Tests：Record round-trip、CRC、尾端不完整、crash-before-sync 與 ack ordering。
4. TCP Runtime Tests：frame partial read/write、連線重建、rpc correlation。
5. Python 3-Process Integration：SIGKILL + restart、網路故障、查詢結果與 logs。
6. Independent Linearizability Checker：對小型 History 枚舉合法 sequential KV semantics；pending/timeout 需要容許可能完成或未完成。

## 必測 Scenario Matrix

| ID | Scenario | 必須觀測 |
|---|---|---|
| T01 | 3-node initial election | 至多一位合法 Leader，2/3 votes |
| T02 | simultaneous candidates | 不會有雙 Leader / split vote 可重試 |
| T03 | outdated candidate | freshness rule 不給票 |
| T04 | higher term observed | step down + persist term before response |
| T05 | normal Put/Delete/Get | committed-apply and return |
| T06 | leader SIGKILL | majority eventual progress |
| T07 | follower SIGKILL | majority writes continue |
| T08 | minority partition | isolated former leader no successful new op |
| T09 | partition heal | conflict suffix repair & convergence |
| T10 | lagging follower | matchIndex bounds and catchup |
| T11 | prevLogIndex/Term mismatch | rejection, retry, no committed truncation |
| T12 | old term majority | direct old-term commit prohibited |
| T13 | crash before fsync | no acknowledged non-durable mutations |
| T14 | crash after fsync before reply | retry yields same logical result |
| T15 | crash after majority before apply | committed remains safe, unknown client |
| T16 | restart with uncommitted suffix | no premature KV apply |
| T17 | stale/duplicate AE response | cannot inflate matchIndex |
| T18 | stale RequestVote / Heartbeat | no term/regression or spurious timer reset |
| T19 | duplicated client request | single effect, original result |
| T20 | same id different payload | deterministic request ID reuse error |
| T21 | stale leader GET | no stale successful read |
| T22 | WAL truncated tail and corruption | incomplete tail truncate / corruption fail closed |
| T23 | fragmented TCP frames | no parse/alloc bug |
| T24 | disk error, disk full | fail closed, no success ack |
| T25 | all nodes restart with durable logs | safe replay after relearning commit index |
| T26 | randomized 1000 seeds | zero invariant violations, reproducible failures |

## Gate Discovery

`gates.json` 的 `rust_tests` 使用 unique identifier，`verify_gate.py` 會先收集 `cargo test --workspace -- --list` 並確認指定名字存在。這**不取代**測試正確性的人類 code review；禁止用空測試混過 gate。

## Linearizability Oracle

History 具有 `client, request_id, operation, invocation_time, response_time/status, result`，必須考慮 real-time order：A 的成功 response 在 B invocation 前 ⇒ A 在順序中先於 B。並行操作可重排；Unknown outcome 的 pending PUT 可能未執行，也可能已完成，不能一律當失敗。每個操作使用 sequential reference KV model + replicated session semantics；對小歷史做 backtracking；為效能把不同 key 分組需先論證與 Session Semantics 相容。

## Liveness Oracle

在「固定時間內網路穩定、至少 2 nodes 運作、磁碟正常、沒有其他 fault」的條件下量測**最終進展**；這是測試條件下的 liveness 證據，不是 Raft 的固定選舉時間保證。未達進展時保留 logs、fault schedule、seed。

## Failure triage

需保存 per-node structured trace（term、vote、index、rpc_id、durable seq、commitIndex、lastApplied）、WAL dump、client history、fault schedule、seed、首次違規 invariant 與重現指令。


---

<!-- Source: docs/05_RECOVERY_CLIENT_SEMANTICS.md -->

# 05. Recovery, Replicated Read, Retry & Session Semantics

## 1. Restart Sequence

```
stop/restart -> verify node.meta identity
             -> scan/check WAL txns
             -> restore durable Term/Vote/Log only
             -> reset role=Follower, commit=0, lastApplied=0, KV=empty, Session=empty
             -> listen peers / restart election timer
             -> receive legitimate leader AE with commit info OR elect leader and commit new-term NoOp
             -> apply only confirmed prefix in index order
             -> serve successful client requests only on safe Leader commit/apply path
```

**嚴禁**把 WAL Log 本身當成所有 Entries 已經 Commit 的證據。新 Leader 應先 append 本 Term NoOp，持久化及提交之後才可推進早期合法 Prefix 的確認。Follower 的 `leaderCommit` 通知也必須綁定當次已匹配的 Prefix。

## 2. Session Model

- ClientId: stable random u128（由 Client 生成）；Sequence: 0..n 嚴格遞增；**同 client 僅一筆 outstanding logical request**。
- Session table key 為 `(client_id, sequence)`，V1 保存完整歷史，不做 session GC（避免先驗證不了舊 request）。Record 存 canonical command bytes 或其強校驗 digest + 原始 deterministic result。
- 每個 Replicated `Put/Delete/Get` 對應 ClientId/Sequence；`NoOp` 不含 ClientId。
- Apply 首次看到 request：先以已 committed map 狀態執行，保存 result，再回傳。
- Apply duplicate 且 command 相同：無副作用，返回原結果；若 command 不同：`REQUEST_ID_REUSE`，不修改 KV。
- 重試在不同 Leader 可能形成**多筆 Log Entries**，但 Session Table 應保證同一 logical request **只 Apply 一次效果**。這不是「exactly one log entry」。
- 若 Client 將舊 request ID 用作新的邏輯讀取，返回舊結果是 duplicate semantics，不可宣稱是新的線性化 read；新 logical GET 必須分配新 sequence。

## 3. GET Linearizability

V1 每個新 GET 都進入 Log。Leader 必須能 Commit/Apply 該 GET entry 才返回值，這讓線性化點落在該 entry 的 Commit / 序列順序。舊 Leader 無 Quorum 無法提交新 GET。任何 duplicate result 回覆也不能繞過 Leader safe path，避免 partitioned leader 的快取結果被誤當成新讀取。

**重要區別**：Log append 本身不是線性化點；Client 成功只在 committed apply 後，已滿足 real-time ordering（在單 client 一個 pending logical op 的限制下）。

## 4. Timeout / Uncertain Write

```
PUT(k=v, id=21) -> quorum persisted -> leader crash BEFORE reply
Client timeout -> UNKNOWN_OUTCOME
Client retries SAME id=21 -> committed state replay -> same original result
```

相同 ID 不重複副作用。不同 ID 的新 PUT 則是新的合法操作（可能再覆寫 KV）；不可以把原本 unknown write 直接換 ID 重送。

## 5. Client Error Contract

| Status | Meaning |
|---|---|
| OK | Leader 已合法 Commit 並 Apply 對應 Command；回傳 deterministic result |
| NOT_LEADER | Node 不接受此新操作；可提供非權威的 leader hint |
| TIMEOUT_UNKNOWN | Request 結果未知；以原 ID 重試 |
| REQUEST_ID_REUSE | 相同 ClientId/Seq 但不同內容 |
| INVALID_ARGUMENT | Command 尺寸、型別、key 等不合約 |
| STORAGE_ERROR | Node 無法提供 durability guarantee；不回成功 |

## 6. V1 Non-goals

ReadIndex、Leader Lease、Follower Stale Read、Snapshot、Session TTL/GC、並發同一 Client 多 request；這些都會增加 correctness proof burden，不得偷偷加進 V1。


---

<!-- Source: docs/06_ENVIRONMENT_OPERATIONS.md -->

# 06. Environment & Operations

## 1. Windows 11 + WSL2 Ubuntu

- 將 repo 與 WAL 放在 `~/projects/...` (WSL Linux filesystem)，不要用 `/mnt/c` 做 durability 或 latency 結論。
- 必須確認 `rustc`, `cargo`, `rustfmt`, `clippy`, `python3`, `pytest`, `git`, `codex` 在 **WSL Ubuntu shell** 可執行。
- 先完成 `codex login`。如 Cargo dependency fetch 受 Codex sandbox 網路限制，**使用者可在外部預先 `cargo fetch`**，而不是啟用危險的 `--yolo`。
- 每個 Process 採單獨 `data/node-{1,2,3}`；不要使用同一 WAL 目錄。

## 2. Initial runtime settings

```
peer ports:       7101 / 7102 / 7103 (via proxy endpoints in fault tests)
client ports:     7201 / 7202 / 7203
heartbeat:        100 ms
election:         uniform random 400–800 ms
rpc timeout:      250 ms
client timeout:   3000 ms
append batch:     32 entries
frame limit:      1 MiB
key/value bounds: 256 / 4096 bytes
```

這是初始測試參數，不是 timing guarantee。Time 對 Safety 不應有影響；Liveness 與性能需基於實際 trace 調整。

## 3. Tool Commands

```
bash scripts/preflight.sh
bash scripts/run_phase.sh 00
python3 scripts/verify_gate.py 00
bash scripts/run_phase.sh 01
# ...
```

Codex CLI 一階段執行以 `codex exec --sandbox workspace-write --cd <repo> -` 從 stdin 讀取 phase prompt。執行產生的輸出保存在 `artifacts/codex/`。若未安裝 Codex，僅能閱讀 spec，不等於已執行。

## 4. Fault Proxy

Proxy 需按 A->B 與 B->A 不同方向處理**完整 frame**：allow/drop/delay/duplicate/reorder，並能切斷現存連線，否則測試中的隔離可能不完整。Packet-bytes 在 TCP stream 裡面不保證 frame boundary；proxy 需先解析 framing。

## 5. SIGKILL Crash and Restart

Python subprocess 監控 Process PID、stdout/stderr、退出代碼。Crash 使用 `SIGKILL` 而非 graceful shutdown；Restart 使用**原資料目錄**。每次可留 Node WAL dump、RPC trace、Client history。

## 6. Reporting and Maintenance

每天或每次 Phase 完成，保存 Git checkpoint；變更協定/格式須更新 ADR、paper mapping 及 tests。WAL 無 compaction，需設定測試上限、磁碟空間監控。**不能**為了加快性能跳過 fsync 或回覆前屏障。


---

<!-- Source: docs/07_IMPLEMENTATION_ROADMAP.md -->

# 07. Phase Roadmap and Acceptance Gates

| Phase | Scope | 最主要的證據 |
|---|---|---|
| P00 | Workspace、Contracts、Seeded simulator skeleton | 同 Seed 可重現；三節點 Quorum 定義正確 |
| P01 | Election/Term/Vote | Split vote、Log Freshness、持久投票意圖 |
| P02 | Log Replication、Commit | Conflict、Current-Term Rule、stale ACK |
| P03 | WAL、Crash Recovery | fsync-before-ACK、Vote Restart、未提交不能 Apply |
| P04 | TCP Transport、3 Process Runtime | partial frame、restart smoke、rpc correlation |
| P05 | KV、Client Session、Replicated GET | Retry 去重、RequestID reuse、線性化讀 |
| P06 | Fault Proxy、Partition Matrix | partitioned leader cannot commit、heal convergence |
| P07 | History Checker、Random Verification | Linearizability oracle、Unknown outcome、seed replay |
| P08 | Benchmarks、Debug Evidence、Final Docs | p50/p95/p99、failover/recovery、Repro report |

## 依賴順序

```
P00 -> P01 -> P02 -> P03 -> P04 -> P05 -> P06 -> P07 -> P08
```

每一步先執行 `codex/PHASE_XX_*.md`，再執行 `python3 scripts/verify_gate.py XX`。Gate 會執行 format、clippy、完整 Rust tests、指定 Python tests，並驗證各 Phase 指定的 Rust test 名稱已被發現。若有失敗，即使 Codex 自行宣稱完成也不能開始下一 Phase。

## 粗估工作量（不是交付時間承諾）

在一位已熟悉 Rust 的工程師使用 Codex 輔助、持續 Code Review 的前提下，第一版可能需要 **80–160 工程小時**。影響最大的未知數是 WAL Crash Semantics、Linearizability Checker 及真實 Proxy 的故障重現。若是 Rust 初學者或要求更強形式化證明，時間會明顯增加。這是容量規劃上的粗估，不是外部測量或完成時間保證。

## 請不要做的事情

- 遇到 P02 current-term commit bug 不可先做 KV UI 略過。
- 遇到 P03 fsync Failpoint 不可用 sleep/retry 掩蓋未 durable ACK。
- Python integration tests 不得使用新的簡化 Consensus Core。
- P08 不可將性能 benchmark 當成放寬 Safety Gate 的理由。

## 風險分布

- 最高：P02/03/05/07；Safety Critical。
- 中：P01/04/06；Concurrency & Network Integrations。
- 較低：P00/08；仍須可靠測試與文件。


---

<!-- Source: docs/08_RAFT_PAPER_TRACEABILITY.md -->

# 08. Raft Paper and Codex Reference Traceability

## Primary authority

**Diego Ongaro and John Ousterhout, “In Search of an Understandable Consensus Algorithm (Extended Version)”**, May 20, 2014, https://raft.github.io/raft.pdf

| Paper section | Required project behavior | Gate |
|---|---|---|
| Figure 2, Persistent State | term, vote, log persisted before dependent RPC response | P01/P03 |
| Figure 2, RequestVote | term check and log freshness check | P01 |
| Figure 2, AppendEntries | `prevLog` check, conflicting suffix, commitBound | P02/P03 |
| Figure 2, Leaders | current-term commit predicate, nextIndex/matchIndex | P02 |
| Figure 3 | Election Safety, Leader Append-Only, Log Matching, Leader Completeness, State Machine Safety | P01/P02/P07 |
| §5.2 | randomized election, split vote, one vote per term | P01 |
| §5.3 | log replication and conflict repair | P02 |
| §5.4.1 | Election Restriction / up-to-date candidate | P01 |
| §5.4.2 / Figure 8 | cannot directly commit previous-term entry by counting replicas | P02 |
| §8 | client retry & read-only consistency safety | P05/P07 |

Important nuance: Figure 8 uses five nodes as a pedagogical counterexample. V1 runtime remains three processes. Implement current-term predicate regardless; explain paper's full counterexample in tests/docs, and optionally use a pure isolated 5-voter test fixture (not a production 5-process deployment) to demonstrate it.

## Supplementary authoritative reference

Diego Ongaro, *Consensus: Bridging Theory and Practice* (dissertation), https://ongardie.net/var/blurbs/pubs/dissertation.pdf . Read-only queries require current-term committed entry, quorum confirmation and applied state when bypassing the log. V1 deliberately **does not bypass the log**.

## Codex CLI reference

- CLI and `codex exec`: https://learn.chatgpt.com/docs/cli/reference
- Non-interactive execution: https://learn.chatgpt.com/docs/non-interactive-mode
- CLI quickstart: https://learn.chatgpt.com/docs/cli

The helper script uses `codex exec --sandbox workspace-write --cd <repository> -` and avoids unrestricted sandbox bypass. Verify locally using `codex exec --help`; CLI versions can change.


---

<!-- Source: docs/09_DECISIONS_ADR.md -->

# 09. Architecture Decision Records

- **ADR-001 / Accepted**: Rust stable over C++. Justification: fewer memory/concurrency incidental hazards; consensus safety still needs independent verification.
- **ADR-002 / Accepted**: Single owner actor for Raft, multithreaded I/O through channels. Lower correctness surface than per-RPC shared mutex mutation.
- **ADR-003 / Accepted**: Sync WAL durable barrier in event loop for V1. Prioritize correct persistent ACK over throughput.
- **ADR-004 / Accepted**: Single in-flight AppendEntries per peer for V1. Easier stale response handling; pipeline optimization later.
- **ADR-005 / Accepted**: Replicated GET; read result produced by committed-apply sequence. ReadIndex optimization deferred.
- **ADR-006 / Accepted**: Full session history in memory, reconstructed from committed log. No session garbage collection until snapshot design.
- **ADR-007 / Accepted**: Volatile commitIndex/lastApplied on restart; committed-knowledge relearned. No premature replay of WAL entries.
- **ADR-008 / Accepted**: Versioned WAL records with CRC and explicit truncate+append transaction; fail closed on ambiguous corruption.
- **ADR-009 / Accepted**: JSON framed TCP with 1MiB frame cap; protocol parsing bounded. Binary codec deferred.
- **ADR-010 / Accepted**: Virtual-time seeded simulator using exact production core. No duplicate algorithm.
- **ADR-011 / Accepted**: Directional frame-level proxy for real-process faults; no direct test bypass.
- **ADR-012 / Accepted**: Fixed 3-node single group; no Dynamic Membership, Snapshot, Multi-Raft, Sharding, TLS, UI.

Any change to an Accepted ADR requires explanation, tests and design review. It must not be an opportunistic shortcut for bypassing a failing acceptance Gate.


---

<!-- Source: docs/10_RELEASE_ACCEPTANCE.md -->

# 10. Final Go / No-Go Acceptance

Release is **NO-GO** unless every gate P00..P08 passed **on the user's WSL2 machine** and reviewer inspected the evidence.

## Release Checklist

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes with discovered required named tests
- [ ] Python three-process integration tests pass
- [ ] deterministic seeded simulation reproducible (>=1000 seeds as stress target)
- [ ] no violations of Figure 3 invariants in recorded runs
- [ ] stale/partitioned leader cannot return successful new PUT/GET
- [ ] fsync/commit/reply crash boundaries tested, no early ACK
- [ ] RequestVote durability under restart verified
- [ ] lagging/divergent follower repairs uncommitted suffix
- [ ] same client request retry has one state machine effect and stable response
- [ ] linearizability checker tested against intentionally invalid history
- [ ] benchmark and failover numbers recorded with environment, sample count and raw results
- [ ] architecture ADR, paper mapping and failure analysis updated
- [ ] reproduction command + seed + fault schedule provided for failures

## Known limits that MUST remain documented

3 fixed nodes tolerate at most one unavailable node while continuing commits; no snapshot/compaction, dynamic membership, TLS, byzantine protection, disk corruption guarantees or universal power-loss durability promise. Unstable network can prevent liveness. Benchmarks of WSL2 single machine are not network-distributed performance predictions.

## Example evidence layout after implementation

```
.gates/phase-00.json ... phase-08.json
artifacts/codex/phase-00.log ...
artifacts/reports/sim-seeds.json
artifacts/reports/crash-boundaries.json
artifacts/reports/linearizability.json
artifacts/reports/benchmark.json
artifacts/traces/<seed-or-run-id>/node-{1,2,3}.jsonl
```

Evidence files above are **future output expectations**, not files currently provided or proven to exist.


---

<!-- Source: codex/HOW_TO_RUN.md -->

# Codex Execution Instructions

Each phase is a direct, standalone implementation task specification, not a vague planning prompt. Run `bash scripts/run_phase.sh 00`, then the next phase **only after** a successful local gate.

The runner feeds phase specification to Codex with:

```
codex exec --sandbox workspace-write --cd "$(pwd)" -
```

Prompts instruct Codex to read `AGENTS.md` and architecture documents. The runner does not use `--yolo`; it captures the CLI log and runs the gate in the user's WSL shell. The gate needs Rust/Cargo/pytest installed. If sandbox prevents cargo dependency resolution, run `cargo fetch` **outside** Codex sandbox and rerun, rather than disabling sandbox.

`gates.json` is an independent acceptance contract. `scripts/verify_gate.py` checks required files, Rust test discovery, `cargo fmt`, `cargo clippy`, `cargo test`, exact Python pytest nodes and required evidence report properties. It writes `.gates/phase-XX.json` only on success and fingerprints the source tree for the next phase's prerequisite check. A gate receipt is necessary but does not replace human code review.

Never run all nine phases in a single unreviewed prompt. Record a Git checkpoint and review the diff after each phase.


---

<!-- Source: codex/PHASE_00_foundation_and_deterministic_harness.md -->

# P00 — Foundation & Deterministic Harness

**Execute:** `bash scripts/run_phase.sh 00`

**Prerequisite:** None (fresh repository).

## Objective

Create a fresh Cargo workspace with packages `raft-protocol`, `raft-core`, `raft-sim`. Freeze IDs, log indexing and event/effect vocabulary; build a virtual-time seeded event queue that can be unit tested **without** an actual Raft election. Do not write production TCP/WAL/clients yet.

## Allowed deliverables

- Root `Cargo.toml` and optional `Cargo.lock`; `crates/raft-protocol`, `crates/raft-core`, `crates/raft-sim`.
- `tests/` test fixtures as appropriate; `docs/API_CONTRACT.md` with exact Rust type signatures and error variants.
- Minimal deterministic event generator/scheduler and `MockDisk` explicitly separate durable/volatile state.

## Ordered execution steps

1. Define immutable cluster configuration for IDs 1,2,3; quorum constant 2, sentinel index=0, entry indexing from 1. Define `Role`, `Term`, `LogIndex`, `LogEntry`, and `Command` including NoOp, Put, Delete, Get as pure protocol data. Reject node IDs outside {1,2,3}.
2. Model RPC messages as strongly typed serde structs, not via arbitrary JSON in core. Declare `Event`, `Effect` and storage barrier identifiers; document how persisted effects order with Send/Reply/Apply.
3. Design `RaftCore` with no socket/fs/Instant calls. Core may be partial/inert at P00; do not fake a leader election.
4. Create simulator queue with monotonic logical `(virtual_time, sequence)` ordering; make pseudo-random choices seed-driven and reproducible. Include `deliver/drop/delay/duplicate` scheduling hooks.
5. Add an independent trace recorder and basic quorum/config assertions. Document assumptions and test invocation.

## Mandatory real Rust tests

- `simulation_seed_is_replayable`: same seed+same scripted events => identical ordered trace (including ties).
- `quorum_is_two_of_three`: exactly 2/3 voters form majority, 1/3 does not.

## Boundary / acceptance

`cargo test --workspace` passes; `cargo test --workspace -- --list` discovers both names. No TCP, no persistence claim, no production Raft logic yet. **Do not** simulate election by writing a second consensus implementation.

## Obligations shared by every phase

1. Read `AGENTS.md`, `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the appropriate earlier code/tests before editing.
2. Do **only** this phase's scope. Do not add a Raft library or introduce multi-node deployment beyond the 3 fixed nodes.
3. Keep code `cargo fmt` clean, `cargo clippy -D warnings` clean, all earlier tests passing; preserve documented types and interfaces.
4. Add **real** negative tests; named test requirements are specified below and in `gates.json`. Do not create empty/stub tests and do not weaken assertions.
5. If a design assumption is impossible, write `docs/BLOCKERS.md` explaining the precise failing invariant and stop instead of silently altering safety contracts.
6. End with a summary of modified files, invariants addressed, commands attempted and genuine outcomes. Do not claim a gate passed unless it actually did.
7. Do not edit `AGENTS.md`, `gates.json`, the provided gate checker or runner to make a failure pass.

## Gate commands (run from workspace root)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/verify_gate.py 00
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.


---

<!-- Source: codex/PHASE_01_election_term_and_vote_safety.md -->

# P01 — Election, Term & Vote Safety

**Execute:** `bash scripts/run_phase.sh 01`

**Prerequisite:** Previous phase P00 passed and has fresh gate receipt.

## Objective

Complete RequestVote and role transitions inside `raft-core` and simulation. Treat durable intent as an effect barrier already in core even though P03 adds physical WAL.

## Allowed deliverables

- `crates/raft-core/src/*`, related protocol structs if needed, `crates/raft-sim` election fixtures.
- Updated `docs/API_CONTRACT.md` on exact Event/Effect declarations.

## Ordered execution steps

1. Implement Role Follower/Candidate/Leader transitions and randomized election timeouts via supplied simulation seed/runtime-generated durations. Use monotonically increasing timer generation to ignore stale timeouts.
2. On Candidate timeout: term increment, vote self, emit `PersistTxn(term,vote)` before requesting votes (dependency barrier). Do not count self vote as durable until storage completion.
3. On RequestVote: stale term reject; higher term durable update and stepdown; enforce votedFor uniqueness and candidate lastLogTerm then lastLogIndex freshness ordering. Grant vote only with durable barrier.
4. On responses: ignore stale term/rpc id, step down on higher term, become Leader only with 2/3 votes. Initialize next/match as Raft Figure 2; no illegal same-term double leader.
5. On receiving same-term valid AppendEntries as Candidate, yield to Follower; stale AppendEntries never extends election timer.
6. Implement immediate current-term NoOp append intent on becoming Leader, but leave full replication to P02; persist intent before sending related RPC.

## Mandatory real Rust tests

- `single_leader_per_term`: directed split-vote and 100 seeded race traces never give two quorum winners in one term.
- `stale_candidate_rejected`: latest term/index ordering for RequestVote, including longer old-term Log.
- `vote_persist_intent_before_reply`: effect order ensures grant and self-vote RPCs depend on durable transaction.

## Boundary / acceptance

No TCP yet; first leader election verified only in actual core used by sim. The test may inspect durable-intent effect ordering without asserting physical fsync (P03 handles that).

## Obligations shared by every phase

1. Read `AGENTS.md`, `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the appropriate earlier code/tests before editing.
2. Do **only** this phase's scope. Do not add a Raft library or introduce multi-node deployment beyond the 3 fixed nodes.
3. Keep code `cargo fmt` clean, `cargo clippy -D warnings` clean, all earlier tests passing; preserve documented types and interfaces.
4. Add **real** negative tests; named test requirements are specified below and in `gates.json`. Do not create empty/stub tests and do not weaken assertions.
5. If a design assumption is impossible, write `docs/BLOCKERS.md` explaining the precise failing invariant and stop instead of silently altering safety contracts.
6. End with a summary of modified files, invariants addressed, commands attempted and genuine outcomes. Do not claim a gate passed unless it actually did.
7. Do not edit `AGENTS.md`, `gates.json`, the provided gate checker or runner to make a failure pass.

## Gate commands (run from workspace root)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/verify_gate.py 01
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.


---

<!-- Source: codex/PHASE_02_log_replication_conflict_repair_and_commit.md -->

# P02 — Log Replication, Conflict Repair & Commit

**Execute:** `bash scripts/run_phase.sh 02`

**Prerequisite:** Previous phase P01 passed and has fresh gate receipt.

## Objective

Implement full AppendEntries receiver, leader per-peer replication index management, current-term-only direct commit, and ordered Apply intents. This is the central Consensus Safety Gate.

## Allowed deliverables

- `raft-core` replication/commit logic, `raft-protocol` AE fields and `raft-sim` directed tests.
- `docs/COMMIT_RULE_REVIEW.md` with Figure 8 discussion and commit predicate.

## Ordered execution steps

1. Define log sentinel index 0; implement checks for leader term, prevLogIndex existence and prevLogTerm matching.
2. Follower: detect conflict only against new entries, atomic `truncate_from+append` transaction intent, no overwrite of matching entries. `leaderCommit` advances only to prefix confirmed by **this** AE; never apply follower's unmatched suffix.
3. Leader: `nextIndex` init lastIndex+1; self `matchIndex` includes durable entries only; peer `matchIndex` begins 0. One in-flight AE/peer; verify `rpc_id`, term and leader epoch, ignore stale replies.
4. Success reply updates peer confirmed match *at most* `prevLogIndex+entries.len()` (heartbeats can confirm prevLogIndex); do not accept remote reported last index as arbitrary evidence. Failure retries earlier prefix, preserving known match bounds.
5. Compute Leader commitIndex only on current term entry N with 2 durable matches; earlier prefix commits indirectly. Ensure Apply effects follow increasing index and never exceed commitIndex.
6. Add current-term NoOp replication on leader election, currentTerm changes/stepdown while replies are inflight, delayed success/failure scenarios.
7. Include explicit property/test that an old-term entry replicated on majority cannot by that fact alone advance leaderCommit; document 5-node paper Figure 8 counterexample without creating production 5-node deployment.

## Mandatory real Rust tests

- `previous_term_entry_requires_current_term_commit`: negative and positive cases; majority old term alone must fail, current-term NoOp permits prefix commit.
- `conflicting_suffix_repaired`: divergent follower suffix is replaced without deleting committed prefix.
- `append_response_cannot_inflate_match_index`: stale/duplicate/empty heartbeat response cannot claim arbitrary follower last index.

## Boundary / acceptance

All five Figure 3 invariants instrumented in sim; absence of a specific `log[N].term == currentTerm` condition is an **automatic No-Go**.

## Obligations shared by every phase

1. Read `AGENTS.md`, `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the appropriate earlier code/tests before editing.
2. Do **only** this phase's scope. Do not add a Raft library or introduce multi-node deployment beyond the 3 fixed nodes.
3. Keep code `cargo fmt` clean, `cargo clippy -D warnings` clean, all earlier tests passing; preserve documented types and interfaces.
4. Add **real** negative tests; named test requirements are specified below and in `gates.json`. Do not create empty/stub tests and do not weaken assertions.
5. If a design assumption is impossible, write `docs/BLOCKERS.md` explaining the precise failing invariant and stop instead of silently altering safety contracts.
6. End with a summary of modified files, invariants addressed, commands attempted and genuine outcomes. Do not claim a gate passed unless it actually did.
7. Do not edit `AGENTS.md`, `gates.json`, the provided gate checker or runner to make a failure pass.

## Gate commands (run from workspace root)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/verify_gate.py 02
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.


---

<!-- Source: codex/PHASE_03_wal_durable_barriers_and_crash_recovery.md -->

# P03 — WAL, Durable Barriers & Crash Recovery

**Execute:** `bash scripts/run_phase.sh 03`

**Prerequisite:** Previous phase P02 passed and has fresh gate receipt.

## Objective

Add `raft-storage` append-only versioned WAL and connect real storage durability boundaries with raft core effect execution. No successful vote/append RPC result can depend on non-durable state.

## Allowed deliverables

- `crates/raft-storage` + tests; integration with runtime-agnostic effect executor in `raft-sim`.
- `docs/STORAGE_FORMAT_V1.md` with exact byte offsets/version/checksum/crash response matrix.

## Ordered execution steps

1. Implement format v1: magic/version/record_type/length/seq/payload/CRC32C, bounded size, deterministic encoding with explicit hard state and truncate+append transaction. Sentinel index is virtual, not WAL entry.
2. Implement `sync_all` on each successful transaction before satisfying dependent Raft effects. Initial identity metadata validates node/cluster/format. Correctly report write/sync failures without sending success ACK.
3. Recovery scans records, verifies CRC, sequence and length; applies each atomic transaction to *persistent Term/Vote/Log only*. Truncate solely provable incomplete tail; fail closed on ambiguous/in-middle corruption.
4. On Restart set Follower, volatile indexes 0, KV/session empty. No Apply until consensus establishes committed prefix; leader's current-term NoOp ensures knowledge of old prefix before servicing clients.
5. Create failpoints before write, after write before sync, after sync before ACK, after quorum before apply, after apply before client reply. Fault simulation must allow an unacknowledged complete WAL transaction to survive a crash.
6. Use fsync-success hook to mark local durable match; do not update local durability based on mere in-memory append.

## Mandatory real Rust tests

- `vote_after_restart_not_duplicated`: after durable vote and restart a different candidate in same term is rejected.
- `append_ack_after_durable_sync`: ACK effect is impossible until verified WAL fsync completion, including forced sync failure.
- `uncommitted_suffix_not_applied_after_recovery`: restart has persistent log but zero committed KV Apply until learned safely.
- `truncated_wal_tail_safe`: partial tail recovery doesn't invent a record; non-tail corruption fails closed.

## Boundary / acceptance

Do not treat un-fsynced means definitely lost. No direct KV replay from log length. WAL round-trip and crash tests pass on Linux filesystem.

## Obligations shared by every phase

1. Read `AGENTS.md`, `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the appropriate earlier code/tests before editing.
2. Do **only** this phase's scope. Do not add a Raft library or introduce multi-node deployment beyond the 3 fixed nodes.
3. Keep code `cargo fmt` clean, `cargo clippy -D warnings` clean, all earlier tests passing; preserve documented types and interfaces.
4. Add **real** negative tests; named test requirements are specified below and in `gates.json`. Do not create empty/stub tests and do not weaken assertions.
5. If a design assumption is impossible, write `docs/BLOCKERS.md` explaining the precise failing invariant and stop instead of silently altering safety contracts.
6. End with a summary of modified files, invariants addressed, commands attempted and genuine outcomes. Do not claim a gate passed unless it actually did.
7. Do not edit `AGENTS.md`, `gates.json`, the provided gate checker or runner to make a failure pass.

## Gate commands (run from workspace root)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/verify_gate.py 03
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.


---

<!-- Source: codex/PHASE_04_linux_tcp_transport_and_three_processes.md -->

# P04 — Linux TCP Transport & Three Processes

**Execute:** `bash scripts/run_phase.sh 04`

**Prerequisite:** Previous phase P03 passed and has fresh gate receipt.

## Objective

Run the same core in three separate WSL2 Ubuntu processes with TCP-based RPC communication; allow real election, replication, persistent restart, and status inspection. KV client API can remain read-only status/no-op until P05.

## Allowed deliverables

- `crates/raft-transport`, `crates/raft-node`, `config/cluster.toml`.
- `tests/integration/test_cluster_smoke.py` using Python subprocess and stable temporary directories.

## Ordered execution steps

1. TCP framing is `u32_be length + JSON`, max 1MiB. Accept partial reads/writes and multiple frames per connection; reject malformed/too-large frames before unbounded allocation.
2. Single-threaded Raft actor owns role, term, log, indexes; socket receivers/timers enqueue events. Actor executes persistent barrier before sending dependent RPC/ACK. IO threads must not mutate raft core.
3. Use peer rpc_id, local leader epoch, term and request metadata to associate replies; timeout/reconnect doesn't invent ACKs. One AppendEntries outstanding per peer, even when TCP reconnect occurs.
4. Parse immutable config for 3 nodes, unique peer/client endpoints and unique storage dirs. Use monotonic clock for timers. Expose debug-only status endpoint including term, role, last log, commit, apply and WAL durable seq.
5. Python subprocess launches three nodes, observes election with polling (not hard-coded sleep outcomes), kills the leader via SIGKILL, verifies remaining nodes elect a new leader, restarts old node with same data dir.
6. Keep all side effects in actor; log every RPC term/index, durable ACK and state transition. No new CRUD or alternative replication logic.

## Mandatory real Rust tests

- `tcp_framing_handles_partial_io`: fragmented header/payload, concatenated frames, excessive size all covered.
- `rpc_correlation_ignores_stale_reply`: old connection/term/epoch reply cannot move matchIndex.

## Mandatory real Python test

- `tests/integration/test_cluster_smoke.py::test_three_process_election_and_restart`

## Boundary / acceptance

Cluster must consist of **three independent Linux PIDs**; integration test must read the PID list and verify uniqueness, not pretend simulated nodes are processes. The full test must pass in WSL2 with `pytest` installed.

## Obligations shared by every phase

1. Read `AGENTS.md`, `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the appropriate earlier code/tests before editing.
2. Do **only** this phase's scope. Do not add a Raft library or introduce multi-node deployment beyond the 3 fixed nodes.
3. Keep code `cargo fmt` clean, `cargo clippy -D warnings` clean, all earlier tests passing; preserve documented types and interfaces.
4. Add **real** negative tests; named test requirements are specified below and in `gates.json`. Do not create empty/stub tests and do not weaken assertions.
5. If a design assumption is impossible, write `docs/BLOCKERS.md` explaining the precise failing invariant and stop instead of silently altering safety contracts.
6. End with a summary of modified files, invariants addressed, commands attempted and genuine outcomes. Do not claim a gate passed unless it actually did.
7. Do not edit `AGENTS.md`, `gates.json`, the provided gate checker or runner to make a failure pass.

## Gate commands (run from workspace root)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 -m pytest -q tests/integration/test_cluster_smoke.py::test_three_process_election_and_restart
python3 scripts/verify_gate.py 04
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.


---

<!-- Source: codex/PHASE_05_kv_linearizable_get_and_client_deduplication.md -->

# P05 — KV, Linearizable GET & Client Deduplication

**Execute:** `bash scripts/run_phase.sh 05`

**Prerequisite:** Previous phase P04 passed and has fresh gate receipt.

## Objective

Implement actual Put/Get/Delete on deterministic KV state machine, replicated session table, and safe client transport semantics. All new GETs MUST be replicated log entries.

## Allowed deliverables

- `crates/kv-state`, client protocol/routing under `raft-node`, `tools/kv_client.py`, `tests/integration/test_client_retry.py`.
- `docs/CLIENT_CONTRACT.md` containing exact sequence and timeout semantics.

## Ordered execution steps

1. Define deterministic command parser and bounds: key <=256 bytes, value <=4096 bytes; invalid arguments returned before a log entry is accepted.
2. Implement NoOp/Put/Delete/Get `apply(entry)` in order, remembering full result record per `(client_id,seq)` and canonical request content; replay of committed prefix reconstructs KV+Session.
3. Implement Session Table full retention (no GC). Duplicate command same ID+content returns original result; duplicate ID+different content returns REQUEST_ID_REUSE with no modification.
4. Only one logical in-flight request per client; Python client retains same ID/sequence across retry. Multiple duplicated *log* entries are allowed; state machine effect happens once.
5. New GET always appended and must be committed/applied before success; never serve local Follower/old-Leader reads; no ReadIndex and no lease optimization.
6. Timeout=UNKNOWN_OUTCOME (not automatic failure). Status `NOT_LEADER` may include a hint. Crash after commit before client reply must be retryable safely after leader change.
7. Successful client reply only after valid commitment and **local apply**. Do not use stale leader cache to answer successful requests without safe quorum/commit confirmation.

## Mandatory real Rust tests

- `retry_same_request_has_single_effect`: same request id logged twice has one logical effect and same result.
- `replicated_get_linearizable`: GET read point is committed/applied log order; stale leader lacks quorum cannot finish new GET.
- `same_id_different_command_rejected`: ID reuse with different canonical command is deterministic rejection.

## Mandatory real Python test

- `tests/integration/test_client_retry.py::test_retry_after_leader_crash`

## Boundary / acceptance

Client API is usable on real 3-process cluster; no new GET success on isolated previous Leader. No session cache-only correctness assumptions.

## Obligations shared by every phase

1. Read `AGENTS.md`, `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the appropriate earlier code/tests before editing.
2. Do **only** this phase's scope. Do not add a Raft library or introduce multi-node deployment beyond the 3 fixed nodes.
3. Keep code `cargo fmt` clean, `cargo clippy -D warnings` clean, all earlier tests passing; preserve documented types and interfaces.
4. Add **real** negative tests; named test requirements are specified below and in `gates.json`. Do not create empty/stub tests and do not weaken assertions.
5. If a design assumption is impossible, write `docs/BLOCKERS.md` explaining the precise failing invariant and stop instead of silently altering safety contracts.
6. End with a summary of modified files, invariants addressed, commands attempted and genuine outcomes. Do not claim a gate passed unless it actually did.
7. Do not edit `AGENTS.md`, `gates.json`, the provided gate checker or runner to make a failure pass.

## Gate commands (run from workspace root)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 -m pytest -q tests/integration/test_client_retry.py::test_retry_after_leader_crash
python3 scripts/verify_gate.py 05
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.


---

<!-- Source: codex/PHASE_06_directional_fault_injection_and_multi-process_failures.md -->

# P06 — Directional Fault Injection & Multi-Process Failures

**Execute:** `bash scripts/run_phase.sh 06`

**Prerequisite:** Previous phase P05 passed and has fresh gate receipt.

## Objective

Implement a Python 3 frame-aware fault proxy/harness and run real 3-process fault scenarios without bypassing the proxy. Keep deterministic sim cases as regression tests.

## Allowed deliverables

- `tools/fault_proxy.py`, `tools/cluster.py`, `tests/integration/test_faults.py` and any necessary deterministic fixtures.
- `docs/FAULT_SCHEDULE_FORMAT.md` with seeded fault schedule and replay commands.

## Ordered execution steps

1. Proxy understands length-prefixed complete RPC frames and maintains directional A->B / B->A rules. Provide allow, drop, delay, duplicate, reorder full frames, block existing connections; do not reorder raw bytes within a TCP stream.
2. Ensure that all test peer addresses route through proxy, including reconnect paths; include a topology/route validation test.
3. Implement Python SIGKILL/Restart, leader discovery polling, temporary persistent node directories, per-node JSONL traces and fault schedule.
4. Run directed partitions: isolate leader A from B/C while B<->C remain connected; old leader must not complete new PUT/GET; majority elects and commits; heal eventually repairs divergent uncommitted suffix.
5. Test follower lag/catchup, conflicting suffix, delayed/duplicated AE response, two candidates, restart with previously unknown outcome.
6. Use bounded liveness windows under stability and distinguish liveness timeout from violation of Safety; capture all logs on failure.

## Mandatory real Python tests

- `tests/integration/test_faults.py::test_partitioned_leader_cannot_commit_or_read`
- `tests/integration/test_faults.py::test_heal_recovers_and_converges`
- `tests/integration/test_faults.py::test_fault_proxy_does_not_bypass_route`

## Boundary / acceptance

Each fault case runs **actual** three `raft-node` Linux subprocesses. Controlled network partitions must affect both new and existing connections. No successful stale leader GET/PUT.

## Obligations shared by every phase

1. Read `AGENTS.md`, `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the appropriate earlier code/tests before editing.
2. Do **only** this phase's scope. Do not add a Raft library or introduce multi-node deployment beyond the 3 fixed nodes.
3. Keep code `cargo fmt` clean, `cargo clippy -D warnings` clean, all earlier tests passing; preserve documented types and interfaces.
4. Add **real** negative tests; named test requirements are specified below and in `gates.json`. Do not create empty/stub tests and do not weaken assertions.
5. If a design assumption is impossible, write `docs/BLOCKERS.md` explaining the precise failing invariant and stop instead of silently altering safety contracts.
6. End with a summary of modified files, invariants addressed, commands attempted and genuine outcomes. Do not claim a gate passed unless it actually did.
7. Do not edit `AGENTS.md`, `gates.json`, the provided gate checker or runner to make a failure pass.

## Gate commands (run from workspace root)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 -m pytest -q tests/integration/test_faults.py::test_partitioned_leader_cannot_commit_or_read
python3 -m pytest -q tests/integration/test_faults.py::test_heal_recovers_and_converges
python3 -m pytest -q tests/integration/test_faults.py::test_fault_proxy_does_not_bypass_route
python3 scripts/verify_gate.py 06
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.


---

<!-- Source: codex/PHASE_07_independent_history_checker_and_randomized_verification.md -->

# P07 — Independent History Checker & Randomized Verification

**Execute:** `bash scripts/run_phase.sh 07`

**Prerequisite:** Previous phase P06 passed and has fresh gate receipt.

## Objective

Implement an independent Python linearizability oracle and serious adversarial state-machine simulation, including unknown operations and randomized crash/partition schedule replay.

## Allowed deliverables

- `tools/linearizability.py`, `tests/history/test_checker.py`, `tests/integration/test_randomized.py`.
- `artifacts/reports/verification.json` with real run metadata and precise pass counts; `docs/VERIFICATION_FINDINGS.md`.

## Ordered execution steps

1. Write sequential KV reference semantics in Python **independent from Rust Core**; ingest operation records with invoke, response, status and return value.
2. Check real-time precedence for non-overlapping completed operations; enumerate legal linearizations for small concurrent histories. Pending/unknown writes can be absent or be placed if consistent. Document bounded search limit and what was/was not proven.
3. Add known invalid histories; checker MUST reject a stale read and logically conflicting outcomes. Add known valid histories including pending unknown operation and duplicated retries.
4. Add seeded randomized Raft sim traces (target >=1000 seeds, 10000 events/seed as tunable baseline) and invariant check I1..I5. Preserve seed, event trace and first failing invariant.
5. Real 3-process integration: mix concurrent clients, faults, crash/restart, delayed frames; capture history and run checker. Confirm no Safety failures across executed runs; liveness measured separately.
6. Generate `verification.json` with actual executable/version/seed_count/event_count/pass_count/failures and a command to reproduce. No fabricated numbers or empty report.

## Mandatory real Rust test

- `simulation_randomized_invariants`: seed-driven I1..I5 assertions on actual core (not duplicate algorithm).

## Mandatory real Python tests

- `tests/history/test_checker.py::test_linearizability_checker_rejects_stale_read`
- `tests/history/test_checker.py::test_pending_operation_has_unknown_outcome`
- `tests/integration/test_randomized.py::test_seeded_fault_history_is_linearizable`

## Boundary / acceptance

Explicitly list unknown/pending operations, not just success-only histories. If checker fails, save counterexample and **do not** narrow the checker to ignore it.

## Obligations shared by every phase

1. Read `AGENTS.md`, `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the appropriate earlier code/tests before editing.
2. Do **only** this phase's scope. Do not add a Raft library or introduce multi-node deployment beyond the 3 fixed nodes.
3. Keep code `cargo fmt` clean, `cargo clippy -D warnings` clean, all earlier tests passing; preserve documented types and interfaces.
4. Add **real** negative tests; named test requirements are specified below and in `gates.json`. Do not create empty/stub tests and do not weaken assertions.
5. If a design assumption is impossible, write `docs/BLOCKERS.md` explaining the precise failing invariant and stop instead of silently altering safety contracts.
6. End with a summary of modified files, invariants addressed, commands attempted and genuine outcomes. Do not claim a gate passed unless it actually did.
7. Do not edit `AGENTS.md`, `gates.json`, the provided gate checker or runner to make a failure pass.

## Gate commands (run from workspace root)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 -m pytest -q tests/history/test_checker.py::test_linearizability_checker_rejects_stale_read
python3 -m pytest -q tests/history/test_checker.py::test_pending_operation_has_unknown_outcome
python3 -m pytest -q tests/integration/test_randomized.py::test_seeded_fault_history_is_linearizable
python3 scripts/verify_gate.py 07
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.

## Required report schema

`artifacts/reports/verification.json` must have `command`, positive integer `seed_count`, `event_count`, `pass_count`, and `failures: []`, populated from a real executed test.


---

<!-- Source: codex/PHASE_08_benchmark_debug_evidence_and_final_review.md -->

# P08 — Benchmark, Debug Evidence & Final Review

**Execute:** `bash scripts/run_phase.sh 08`

**Prerequisite:** Previous phase P07 passed and has fresh gate receipt.

## Objective

Produce measured baseline/fault performance and complete reproducible engineering evidence. Do not optimize by weakening durability or GET safety.

## Allowed deliverables

- `tools/benchmark.py`, `tests/integration/test_benchmark_smoke.py`, `docs/IMPLEMENTATION_REPORT.md`, `artifacts/reports/benchmark.json`.
- Final revisions to `docs/VERIFICATION_FINDINGS.md`, `docs/FAULT_SCHEDULE_FORMAT.md` as evidence warrants.

## Ordered execution steps

1. Benchmark fixed workloads: sequential single client, 8 concurrent clients, one follower isolated/slow, leader crash and recovery. Record hardware/WSL/CPU/RAM/OS/compiler, payload size, sample count, WAL sync policy and warmup.
2. Measure PUT/GET p50/p95/p99 client latency, operations/sec, WAL fsync p50/p95/p99, failover time, follower catchup time, RSS/CPU/WAL growth. Never invent observations.
3. Implement `test_benchmark_smoke` which verifies report schema and ability to run a tiny real process benchmark without a permanent service.
4. Write `docs/IMPLEMENTATION_REPORT.md`: module map, Figure 2/3/§5.4.2 mapping, commit proof sketch, storage barrier and crash matrix, failure experiments, known limits, command reproduction, changed ADRs.
5. Verify all gates still pass; include negative test demonstration that deliberately violates current-term commit rule in isolated test scope and is detected (not in shipped code).
6. Record final `benchmark.json` and `verification.json`, each with actual command/date/env, and note missing measurements as **NO-GO** rather than filling placeholders.

## Mandatory real Python test

- `tests/integration/test_benchmark_smoke.py::test_benchmark_smoke`

## Boundary / acceptance

Every earlier Gate remains passing, no claim of formal proof or cloud-scale performance. Publish a reproducible final review and known limitations. If benchmark cannot run because of environment, gate must fail honestly until environment permits it.

## Obligations shared by every phase

1. Read `AGENTS.md`, `docs/02_ARCHITECTURE_RFC.md`, `docs/03_PROTOCOL_STORAGE.md`, `docs/04_INVARIANTS_TEST_PLAN.md`, and the appropriate earlier code/tests before editing.
2. Do **only** this phase's scope. Do not add a Raft library or introduce multi-node deployment beyond the 3 fixed nodes.
3. Keep code `cargo fmt` clean, `cargo clippy -D warnings` clean, all earlier tests passing; preserve documented types and interfaces.
4. Add **real** negative tests; named test requirements are specified below and in `gates.json`. Do not create empty/stub tests and do not weaken assertions.
5. If a design assumption is impossible, write `docs/BLOCKERS.md` explaining the precise failing invariant and stop instead of silently altering safety contracts.
6. End with a summary of modified files, invariants addressed, commands attempted and genuine outcomes. Do not claim a gate passed unless it actually did.
7. Do not edit `AGENTS.md`, `gates.json`, the provided gate checker or runner to make a failure pass.

## Gate commands (run from workspace root)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 -m pytest -q tests/integration/test_benchmark_smoke.py::test_benchmark_smoke
python3 scripts/verify_gate.py 08
```

## Required evidence

- Every Rust test ID and exact pytest node ID listed in `gates.json` for this stage exists and passes.
- Previous stages remain green. All new transitions and persistence boundaries have structured trace or assertions.
- Codex final output reports actual commands and results, not forecasts.

## Required report schema

`artifacts/reports/benchmark.json` must have `command`, positive integer `run_count`, nonempty `metrics` object, and nonempty `environment` object. Preserve P07 `verification.json` with valid real run evidence.
