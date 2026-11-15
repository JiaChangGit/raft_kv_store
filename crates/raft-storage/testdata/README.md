# 測試用的固定檔案

`pre-s5-steps.wal`：S5 之前的程式（tag `s1-s4-review-verified`）以 `Wal::save` 逐步寫出的 WAL，
步驟和 `src/tests.rs` 的 `the_wal_bytes_match_a_file_written_before_s5` 相同（取自 S5 審查者的探針 `g6_old.rs`）。
10,336 bytes，sha256 `55de43003e0d9ac8dfef87ce7b4cb2e5d6172fa5bb06120ba06da4e73040d460`，與審查紀錄
（`docs/INDEPENDENT_REVIEW_S5.md` §8）相同。2026-11-15 重新產生：

```bash
git archive s1-s4-review-verified crates config Cargo.toml Cargo.lock rust-toolchain.toml | tar -x -C /某個暫存目錄
# 把探針放到 crates/raft-storage/examples/g6_old.rs，然後在那個目錄執行：
cargo run --release --locked -p raft-storage --example g6_old -- 輸出目錄
```

這個檔案釘住「S5 起改走 `save_delta`，寫出的位元組和 S5 之前完全相同」。格式若要改，這份檔案與測試要一起改，並說明原因。
