# 實驗 A：skill 的觸發與誤觸（執行前登記）

依 [SPEC.md](../../SPEC.md) §3.3，照框架 `evals/README.md`「第一次在真實 host 驗證」的四步驟。

- **情境**（框架 `evals/scenarios.json`，自帶材料的 6 個）：

  | 情境 | 框架預期的主要入口 |
  |---|---|
  | `trace-without-lesson` | `trace-code` |
  | `learning-with-trace` | `guide-learning` |
  | `flaky-repeat-evidence` | `debug-root-cause` |
  | `cleaned-copy-is-not-original` | `read-research` |
  | `object-authorization-and-model-output` | 不需要 skill |
  | `presentation-priority-and-decision` | 不需要 skill |

- **兩組**：`control`（只有材料檔）；`framework`（同樣的材料，加上 `agf project install --host claude --skill trace-code --skill debug-root-cause --skill guide-learning --skill read-research --skill review-change --apply`）。
  目錄在專案之外（暫存目錄），不會從上層繼承本專案的 skill（D0）。
- **提示詞**：框架 `evals/paired/tasks.json` 的 `startup`，空一行，接情境的 `request`。兩組相同，不點名 skill。
- **模型與限制**：`claude-sonnet-5-5`；可用工具只有 `Read Grep Glob Skill`（情境都要求不執行、不改檔）；`--permission-mode dontAsk`；每次預算上限 2 美元、時間上限 900 秒。
- **順序**：第 1、3、5 個情境先跑 control，第 2、4、6 個先跑 framework，避免快取與順序都偏向同一組。
- **量什麼**（從事件流直接取，不靠模型自述）：
  - 觸發：framework 組該用 skill 的 4 個情境，有沒有呼叫預期的 skill；不需要 skill 的 2 個，有沒有呼叫任何 skill（誤觸）。control 組有沒有呼叫使用者層級的 skill（例如 `debug`）。
  - 讀了哪些檔（含 `SKILL.md`、`AGENTS.md`、指引）；被拒的操作。
  - 回合數、耗時、token（輸入、快取建立、快取讀取、輸出）、CLI 估算的費用。
- **成果品質**：依 [盲評的提示詞](../grader_prompt.md)，由看不到組別的 Opus 5.5 子代理依情境的 `observe`／`avoid` 評分（0～8 分，和框架 2026-11-09 的量表一致）。
- **產物**：`results.json`（每次的摘要指標）、`outputs/`（每次的最終回覆）、`grades.json`；原始事件流在 `artifacts/runs/s8-20261116/A/`。
