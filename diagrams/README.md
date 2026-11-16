# V1 時期的兩張圖

這兩張圖在 2026-11-12 的 V1 基準時畫成，`.mmd` 是圖源，`.svg` 是當時渲染的成品。之後沒有重畫：這台 WSL 的 `mmdc` 無法渲染，改圖源會讓兩者對不上。
現在的架構與流程圖在 [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md)。

| 圖 | 現在還對嗎 |
|---|---|
| `put_commit`：一次 PUT 從 client 到回覆 | 這個抽象層級大致還對。S5 之後，leader 會把同一輪的多筆寫入合併成一次 WAL 落盤（group commit），圖裡只畫了一筆 |
| `raft_states`：節點角色的轉換 | 部分過時：「兩個不同合法票」在 S4 之後是「目前設定的多數票」；S4 另外加了「leader 不在新設定裡時，等那筆設定 commit 後退位」，圖裡沒有 |
