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
