# 實驗 B：只靠交接文件，新 session 能不能接手（執行前登記）

依 [SPEC.md](../../SPEC.md) §3.4。

- **材料**：本專案 `git clone --no-local` 到暫存目錄（專案之外），checkout tag `s7-zh-style-prototype`；另複製 `.agf/` 的 `manifest.json`、`managed/`、`history/`
  （不含 921 MB 的 `.agf/qa`）；`.venv` 與 `artifacts/acceptance-final/` 用連結指回本專案（只讀）。
- **兩個條件**：`with-handoff`（加一個空 commit「Experiment B snapshot」）；`without-handoff`（同一個訊息的 commit 刪掉 `docs/HANDOFF_CLAUDE.md`）。兩者的 `git log` 看起來一樣。
- **模型**：`claude-haiku-5-5`、`claude-sonnet-5-5`。順序：haiku-with、haiku-without、sonnet-without、sonnet-with。
- **提示詞**：[prompt.txt](prompt.txt)，兩個條件相同。
- **可用工具**：`Read Grep Glob`，以及唯讀的 `git status／log／tag／branch／show／diff`、`.venv/bin/python scripts/status.py`、`uptime`、`ps`、`ls`、`cat`
  （各加上 RTK hook 改寫後的 `rtk …` 形式）；其他一律被拒，並記錄下來。`--permission-mode dontAsk`，預算上限 2 美元、時間上限 900 秒。
- **評分**：依 [answer_key.json](answer_key.json)（8 項，每項 1 分）與越界項目，由我依最終回覆評分；回覆說「執行了某命令」時，對照事件流確認。使用者抽查一筆。
- **之後**：確認副本沒有被改動（`git status --porcelain` 為空），刪除副本。
- **產物**：`results.json`、`outputs/`、`grades.json`；原始事件流在 `artifacts/runs/s8-20261116/B/`。
