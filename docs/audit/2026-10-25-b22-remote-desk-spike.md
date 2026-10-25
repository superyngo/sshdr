# B22 遠端 Terminal Window 技術驗證（spike）結果
Status: Landed
Date: 2026-10-25
Source: [計畫](../plan/2026-10-25-b22-remote-desk-spike.md)；[ADR 0007](../adr/0007-desktop-tasks-are-workspaces.md)；[ADR 0004](../adr/0004-fleet-registered-machines-with-connect-time-tiers.md)
Code: branch `spike/b22-remote-desk` @ `c006f99d`（不併入 `main`）

## 1. 結論

在 **Server Tier** **Machine** 上，desk 用 SSH 執行與本機相同的 CLI，可以把遠端 tab 開成 **Terminal Window**：
操作、延遲、成本與 **Attention** 都可接受，不需要遠端裝新版 herdr（0.9.5 就能用）。B15 可以沿用這個做法，但有三件事要先處理：

1. **一個 ControlMaster 最多 10 條串流**（sshd `MaxSessions` 預設 10）。超過時 OpenSSH 會為多出來的串流各開一條 TCP 連線，且不會報錯。
2. **斷線時視窗會被關掉再重開**，位置與大小不見。
3. **連線卡住時看不出來**：輪詢沒有逾時，desk 不會顯示 `! offline`。

## 2. 實作摘要

| 檔案 | 內容 |
|---|---|
| `src/cli/desk/transport.rs` | 新增：`Local`（本機執行檔）或 `Ssh`；SSH 選項 `BatchMode=yes`、`ControlMaster=auto`、`ControlPath=platform::shared_ssh_control_path(...)`、`ControlPersist=600`、`ServerAliveInterval=15`、`ConnectTimeout=10`；遠端參數用 POSIX 單引號；stderr 分類成 `! auth` 或 `! offline` |
| `src/cli/desk/server.rs`、`stream.rs` | 列表、建立、改名、關閉與 `terminal session control` 都改經 `transport::herdr()`；輪詢多送一個 `DeskEvent::Link` |
| `src/cli/desk/app.rs` | 視窗標題加主機前綴；工作列提示顯示目標與連線狀態；啟動失敗時印出 `! auth` 或 `! offline`；遠端不讀寫 `desk.json`；`HERDR_DESK_STATS` 多記按鍵到 frame 的延遲 |
| `src/cli/desk.rs` | `herdr desk --ssh <target> --remote-session <name>` |
| `scripts/spike/b22_desk_perf_remote.py` | 遠端效能量測 |

沒有改 upstream 檔案（計畫決策 2A）。

## 3. 環境

| 項目 | 值 |
|---|---|
| 本機 | macOS arm64，`target/release/herdr`（量測）、`target/debug/herdr`（功能） |
| **Machine** | omarchy（Linux x86_64，herdr 0.9.5），經 Tailscale；ping 平均 3.6 ms；經 ControlMaster 跑一個指令 20–30 ms |
| 遠端 Session | 獨立的 `desk-b22`；預設 Session 只在一次錯誤中被碰到（見 §6），已復原 |

## 4. 結果

### 4.1 基本操作（通過）

pty 驅動，遠端 Session 有 Task `api`（2 個 tab，第二個分割）與 `web`：

- 開啟後 3 個視窗，標題 `omarchy: 1`、`omarchy: second (+1 panes)`；0.9.5 沒有 `root_pane_id`，退回規則（第一個沒有 label 的 pane）正確。
- 輸入 `echo hi-b22` 有回應；Start → New window 在遠端建立 `w2:t2`；改名後遠端 `tab list` 看到新名稱（含空白）；最小化、關閉（遠端 tab 跟著關）正常。

### 4.2 輸入延遲（50 鍵，debug build）

| | p50 | p95 | max |
|---|---|---|---|
| 本機 desk | 11.5 ms | 12.4 ms | 12.5 ms |
| 遠端 desk（omarchy） | 24.1 ms | 38.4 ms | 54.7 ms |

遠端多 13 ms 左右，大約是網路往返加上 ssh 多工的成本，打字感覺不到。

### 4.3 串流成本（release，200×60，每個 pane 跑 `seq`，10 秒）

| 視窗 | desk CPU（含 ssh 子程序） | ssh master CPU | 遠端 server 多用的 CPU | frames/s | 解碼後 KiB/s | master 線路 KiB/s | 到 omarchy 的 TCP |
|---|---|---|---|---|---|---|---|
| 1 | 10.8–12.2% | 0.8–1.0% | +7% | 51–56 | 19–21 | 28.8 | 1 |
| 8 | 17.1–17.2% | 1.0% | +5–20% | 45–46 | 14 | 24.1 | 1 |
| 15 | 16.6–17.0% | 0.5–0.7% | +16–20% | 28–32 | 7–8 | — | **7–8** |

- 跟本機比較（B14：1 視窗 2.7%、15 視窗 12.3%）：1 個視窗多出的約 8% 主要是 ssh client 的加解密，視窗變多以後增加得不多。
- 線路流量約為解碼後內容的 1.4 倍（base64 加 JSON）。網卡計數器（`utun5`）會混進其他 Tailscale 流量（不開 desk 時就有 117 KiB/s），所以改用 `nettop` 算 master 程序的流量。
- frames/s 總量不會隨視窗數增加：遠端 server 已經滿載（不開 desk 時 89–122% CPU），每個視窗分到的更新變少。

### 4.4 ControlMaster（有條件通過）

- 8 個視窗：全部串流與輪詢共用 **1 條** TCP（desk 自己的 master）。
- 15 個視窗：多出 6–7 條 TCP。原因是 omarchy 的 sshd `MaxSessions` 預設 10（`/etc/ssh/sshd_config` 只有註解 `#MaxSessions 10`）；master 滿了以後，`ControlMaster=auto` 的 client 直接自己連線。
- 另一個發現：已連上 omarchy 的 herdr client 有自己的 master（pid 27252），desk 的 master（99764）是另一條。`shared_ssh_control_path` 用設定檔路徑加 target 算 hash，兩邊沒有共用到同一個 socket。

### 4.5 斷線

| 情境 | 結果 |
|---|---|
| `ssh -O exit` 關掉 master | 所有串流結束、視窗被關掉；下一次輪詢（≤1 秒）自動建立新 master，視窗重新開啟，可以繼續輸入。**位置與大小遺失**（原本並排，回來變疊放） |
| `kill -STOP` master 33 秒 | desk 本身仍可操作（desk 按鍵有效）；視窗畫面凍結；輪詢卡住，**工作列沒有顯示 `! offline`**；`kill -CONT` 後 4 秒內恢復；離開正常 |

### 4.6 **Attention**（通過）

| 目標 | 結果 |
|---|---|
| `nobody-b22@omarchy…`（沒有金鑰） | 0.15 秒內結束：`Permission denied` → `! auth: … needs SSH authentication; run herdr machine reconnect …` |
| omarchy 的 IP（`known_hosts` 沒有） | 0.08 秒內結束：`Host key verification failed` → `! auth` |

`BatchMode=yes` 讓 desk 不會停在密碼或 host key 提示。分類規則有單元測試。執行中途才變成需要認證的情況沒有實測（要讓 master 失效並撤銷金鑰）。

## 5. 對 B15 的建議

1. **串流數上限**：每台 **Machine** 每個 master 最多 9 條串流（留 1 條給輪詢與指令）；超過時開第二個 master（不同的 `ControlPath`），或只為看得到的視窗開串流（最小化的視窗釋放串流）。長期可在遠端跑一個 bridge，用一條 SSH 承載多條串流，但需要遠端裝新版 herdr。
2. **斷線不關視窗**：串流結束但 tab 還在時，視窗保留位置並標示斷線，連線恢復後在原位重新接上。
3. **輪詢加逾時**（例如 5 秒），逾時就顯示 `! offline`，並改成每台 **Machine** 一條輪詢。
4. **共用 herdr 的 master**：namespace 要跟 herdr client 用同一個（或改用 `write_managed_ssh_config`），否則每台 **Machine** 會有兩條 SSH 連線。
5. `--session` 是全域旗標，會在子命令拿到參數前被拿掉；子命令不要再取同名旗標。

## 6. 執行中的錯誤與處置

- 第一次執行用了 `--session desk-b22`，這個旗標被全域的 Session 解析拿掉（`src/session.rs`），desk 因此連到 omarchy 的**預設 Session**：在 workspace `wB` 建了一個空 tab `wB:t2`，在一個既有 pane 打了 `echo hi-b22`，並以 `--takeover` 暫時接管了那些 pane 的大小（離開時已釋放）。已關閉 `wB:t2`，tab 清單恢復為原本的 `w9:t7, wA:t1, wA:t3, wB:t1`。之後改用 `--remote-session`，而且必填。
- 清理腳本的切片寫錯，把 `desk-b22` 的 8 個 tab 全關了；只影響這個測試用的 Session，已重建。

## 7. 測試後的狀態

- omarchy：`desk-b22` server 已停止；`~/.config/herdr/sessions/desk-b22/` 還在（可刪）。
- 本機：`desk-b22l` server 已停止；`~/.config/herdr-dev/sessions/desk-b22l/`、`/var/tmp/desk-b22-*.json`、`docs/tmp/claude-scratch/b22/` 還在。
