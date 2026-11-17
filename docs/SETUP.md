# 從零開始：安裝、建置、測試、啟動叢集

這份文件從一台還沒裝任何開發工具的 Linux 開始，一步一步做到「三台節點在跑、寫得進去也讀得出來、測試全部通過」。
每一步都寫了預期看到什麼；看到的不一樣時，先看最後一節「常見問題」。

這份文件不需要先懂 Raft；看不懂的名詞（leader、term、voter 等）查 [名詞表](GLOSSARY.md)。

這些步驟在作者的機器上實際執行過：Windows 上的 WSL2、Ubuntu 26.04、Rust 1.99.1、Python 3.14.4（2026-11-17）。
「安裝工具」那一節的 `apt` 與 rustup 命令沒有在一台全新的系統上重做過，其他步驟都在一份全新的 clone 上跑過。

## 1. 需要什麼

- **Linux。** 開發用的是 WSL2 的 Ubuntu；一般的 Ubuntu 或 Debian 也應該可以（沒有試過）。macOS 與 Windows 原生環境沒有試過。
- **專案要放在 Linux 自己的檔案系統。** 在 WSL 裡就是家目錄底下，例如 `~/src/`。不要放在 `/mnt/c/...`：
  那是 WSL 經由網路檔案協定（9P）存取的 Windows 磁碟，`fsync` 很慢，而這個專案每次寫入都要 `fsync`。
- **磁碟空間至少 3 GB**（一次建置約 2 GB）；要跑完整驗收與全部教學的重現，預留 6 GB。記憶體 8 GB 的機器跑完整驗收沒有問題。

## 2. 安裝工具

```bash
sudo apt update
sudo apt install -y build-essential git curl python3 python3-venv
```

- `build-essential`：C 編譯器與連結器，Rust 編譯時需要。
- `python3-venv`：建立 Python 虛擬環境用。測試工具與 client 工具是 Python 寫的。

安裝 Rust 的版本管理工具 rustup：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"
```

第二行讓目前這個 shell 找得到 `cargo`。之後開新的 terminal 通常會自動載入；如果出現 `cargo: command not found`，再執行一次這一行。

不需要自己選 Rust 的版本：專案的 `rust-toolchain.toml` 寫著 1.99.1 與 `rustfmt`、`clippy`，第一次在專案目錄執行 `cargo` 時，rustup 會自動安裝。

## 3. 取得原始碼並建置

```bash
git clone <儲存庫網址> raft-kv
cd raft-kv
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements-dev.txt
cargo build --workspace --locked
```

- `.venv/` 是這個專案自己的 Python 環境，只裝了 `pytest`。之後所有 Python 命令都用 `.venv/bin/python`，不會影響系統的 Python。
- `--locked` 要求使用 `Cargo.lock` 裡記錄的版本，確保你建出來的和作者的一樣。
- 第一次建置要下載相依套件並編譯，約一兩分鐘；最後一行是 `Finished`。

如果你是下載壓縮檔，解壓縮後進入那個目錄即可。這篇以後的命令，都在專案根目錄（`raft-kv/`）執行。

## 4. 跑測試

由快到慢：

```bash
cargo test --workspace --locked            # Rust 的單元與整合測試，約 1 分鐘
.venv/bin/python -m pytest -q tests        # 啟動真實的節點 process，注入網路故障與當機，幾分鐘
```

`cargo test` 會印出很多行 `test result: ok`，每個測試執行檔一行；有一行 `FAILED` 就是有測試失敗。
其中 `crash_process` 那一組最慢（約 1 分鐘），它真的啟動子 process、在寫入途中用 SIGKILL 終止，再檢查重開後的狀態。

`pytest` 會在 `artifacts/runs/` 底下留下每個測試的節點紀錄，方便失敗時查原因；這個目錄不在版本控制內，可以隨時刪掉。

格式與靜態檢查：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

兩個都沒有輸出（或只有 `Finished`）就是通過。

## 5. 啟動一個三台的叢集

`config/cluster.toml` 是位址表：三台節點各用兩個連接埠（port），7101～7103 給節點之間，7201～7203 給 client。
先確認這些連接埠沒有被佔用（`ss -ltn | grep -E ':7[12]0[1-3]'` 沒有輸出就是沒有）。

開三個 terminal，都進到專案根目錄，各執行一行：

```bash
cargo run -q -p raft-node -- --id 1 --config config/cluster.toml --data-dir data/node-1 --bootstrap
cargo run -q -p raft-node -- --id 2 --config config/cluster.toml --data-dir data/node-2
cargo run -q -p raft-node -- --id 3 --config config/cluster.toml --data-dir data/node-3
```

- 只有節點 1 帶 `--bootstrap`：叢集一開始只有它自己一台，它幾百毫秒內就會當選 leader。
- 節點 2、3 以空的資料目錄啟動，還不是成員，只是在等。
- 每台會持續印出 JSON 格式的紀錄，一行一個事件；看不懂沒關係，之後可以導到檔案再查。
- `data/` 不在版本控制內。要重新開始時，先停掉三台，再刪掉 `data/`。

開第四個 terminal，同樣先進到專案根目錄，把節點 2、3 加進成員設定：

```bash
.venv/bin/python tools/kv_admin.py --config config/cluster.toml add 2
.venv/bin/python tools/kv_admin.py --config config/cluster.toml add 3
```

`add 2` 回 `"status": "OK", "result": "1,2"`，接著印一行 `warning`：兩台時任何一台故障，叢集都會停。這是提醒，不是錯誤。
`add 3` 回 `"result": "1,2,3"`。每次加入都要等新節點追上 log，通常一秒內完成。

寫入一個 key，再讀回來：

```bash
ADDRS="--address 127.0.0.1:7201 --address 127.0.0.1:7202 --address 127.0.0.1:7203"
.venv/bin/python -m tools.kv_client $ADDRS --client-id 7 --sequence 0 Put x A
.venv/bin/python -m tools.kv_client $ADDRS --client-id 7 --sequence 1 Get x
```

Put 回 `"status": "OK", "result": null`（`null` 表示 x 原本沒有值）；Get 回 `"result": "A"`。
`ADDRS` 這個變數只在目前的 terminal 有效；換一個 terminal，或用 bash 以外的 shell 時，把三個 `--address` 直接寫出來。

- `--client-id` 是你這個 client 的編號，`--sequence` 是請求的序號，每個新請求加一。用過的 sequence 會被記住：重跑上面兩行時，同一個 sequence 會拿到第一次的結果，不會重新寫入。要重來就換一個 client ID，或把 sequence 接著往後加（下一個是 2）。
- 回覆可能在路上遺失，這時 client 不知道寫入成功了沒有，會收到 `TIMEOUT_UNKNOWN`。用**同一個** client ID、sequence 與內容重送，系統保證同一個寫入只執行一次（[client 契約](CLIENT_CONTRACT.md)）。
- 查看叢集的狀態：`.venv/bin/python tools/kv_admin.py --config config/cluster.toml status`，會列出 leader、term 與成員。

接著可以試試：在節點 1 的 terminal 按 Ctrl-C 停掉它，用 `--sequence 2` 再讀一次 x（另外兩台會選出新的 leader，讀取照樣成功），
然後用同一行命令重新啟動節點 1。帶著 `--bootstrap` 也沒關係：資料目錄裡已經有資料時，這個選項不起作用。
[實際操作](HANDS_ON.md) 用一支腳本把這整個過程跑一遍，並說明每一步看到的數字。

## 6. 完整驗收

驗收把所有測試、mutation 測試、1,000 個種子的模擬、讀寫的效能量測，依 `acceptance/final.json` 的階段順序全部跑一遍，在作者的機器上約 45 分鐘：

```bash
bash scripts/final_acceptance.sh
.venv/bin/python scripts/status.py
```

`status.py` 為每個階段印一行，`PASS [current source]` 表示這個階段對目前的原始碼通過。中途被打斷的話，用同一行命令重跑會從停下的地方接著做。
跑驗收的期間不要改任何原始檔：檔案一變，腳本就會停下來，因為已經產生的結果不再對應目前的程式。
怎麼讀驗收的結果，見 [驗收契約說明](ACCEPTANCE_FINAL.md)。

想確認每篇 bug 教學的重現仍然有效（約 10 分鐘）：

```bash
bash scripts/run_lesson_repros.sh
```

## 7. 常見問題

| 看到的 | 原因與處理 |
|---|---|
| `cargo: command not found` | 這個 shell 沒有載入 Rust 的路徑。執行 `. "$HOME/.cargo/env"` |
| `Address already in use`（節點啟動時） | 連接埠被佔用，可能是上一次的節點沒停。`ss -ltnp \| grep 720` 找出佔用的 process；或複製一份 `config/cluster.toml` 改用別的連接埠 |
| `node directory already owned` | 同一個資料目錄已經有一台節點在用。每台節點要用自己的目錄 |
| `kv_admin.py add` 回 `NOT_LEADER` 或連不上 | 節點 1 還沒當選，等一兩秒再試；或節點 1 沒有帶 `--bootstrap` 啟動 |
| 用 2026-11-17 之前的 tag 建置時，一台的叢集 Get 回 `TIMEOUT_UNKNOWN` | 舊版有這個 bug（[教學 20](lessons/20-a-leader-alone-never-answers-reads/README.md)）。更新到目前的版本 |
| 用 2026-11-17 之前的 tag 建置時，第一次 `cargo test` 有 3 個 raft-node 測試 `File exists`，第二次就過 | 舊版有這個 bug（[教學 21](lessons/21-a-fresh-clone-fails-its-first-test-run/README.md)）。更新到目前的版本 |
| 測試偶爾逾時、效能數字比文件慢很多 | 機器很忙。先看 `uptime` 的負載；WSL 開機時常有 `dockerd`、`containerd` 佔著 CPU |
| 驗收停在 `source changed` | 驗收期間有原始檔被改了。改完再從頭跑 |
| 專案放在 `/mnt/c` 底下，測試非常慢 | 移到 Linux 的家目錄（見第 1 節） |

## 8. 接下來讀什麼

1. [架構說明](ARCHITECTURE.md)：四張圖看懂元件、一次寫入、一次讀取、加入一個成員。
2. [帶讀](LEARNING_GUIDE.md)：跟著一筆 PUT 走過整個系統，附練習。
3. [bug 教學](lessons/README.md)：開發中實際遇到的 21 個 bug。
4. [自問自答](FAQ.md) 與 [名詞表](GLOSSARY.md)：檢查自己懂了沒有。
