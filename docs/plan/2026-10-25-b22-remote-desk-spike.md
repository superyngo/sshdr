# B22 遠端 Terminal Window 技術驗證（spike）計畫
Status: Landed — 結果見 [B22 spike 結果](../audit/2026-10-25-b22-remote-desk-spike.md)；owner 2026-10-25 核准決策 1 可以、決策 2 選 A
Date: 2026-10-25
Source: [ADR 0007](../adr/0007-desktop-tasks-are-workspaces.md)（決策 3）；[ADR 0004](../adr/0004-fleet-registered-machines-with-connect-time-tiers.md)；[B14 計畫](2026-10-25-b14-desk-shell.md)；Backlog B22

## 1. 目標

在真實的 **Server Tier** **Machine** 上驗證：desk 用 SSH 執行與本機相同的 CLI（列表、建立、改名、`terminal session control`），
每個遠端 tab 就是一個 **Terminal Window**。量測輸入延遲與串流成本，確認多條串流共用一條 ControlMaster，
並確認斷線與 **Attention**（需要人工認證）時的行為。結論決定 B15 的做法。

## 2. 前置（已查證）

| 事實 | 出處 |
|---|---|
| 目標 **Machine**：`omarchy.tail83481b.ts.net`（Linux x86_64，herdr 0.9.5 在 `/usr/local/bin/herdr`），已是 saved **Machine**，`machine status` 為 reachable，BatchMode SSH 可登入 | `herdr machine list/status`；實測 |
| 遠端 0.9.5 有 `terminal session control`、`workspace list`、`tab list`；`TabInfo` 沒有 `root_pane_id` | 實測 |
| omarchy 的預設 Session 有你正在用的 workspace | `herdr workspace list`（遠端） |
| `herdr --machine` 轉送會拒絕 `terminal session control` | `src/cli/target.rs` `validate_machine_command` |
| 受管 SSH 選項：`ControlMaster=auto`、`ControlPersist=600`、`ServerAliveInterval=15`；控制 socket 路徑由 `platform::shared_ssh_control_path` 決定（`pub(crate)`）；`write_managed_ssh_config` 是 `remote/attach.rs` 的私有函式 | `src/remote/attach.rs`；`src/platform/unix_common.rs` |
| **Attention** = **Machine** 需要人工動作（SSH 認證、host key）才能連線 | glossary |

## 3. 做法

- 分支 `spike/b22-remote-desk`，從 `main` 開；spike 程式不合併，結論寫進 `docs/audit/`。
- desk 加一層傳輸：`Local`（現在的 `current_exe`）與 `Ssh { target, session }`。
  `Ssh` 執行 `ssh -T <受管選項> <target> env HERDR_SESSION=<session> herdr <args>`；列表、建立、改名、關閉與串流都走它。
- 受管選項：desk 自己組 `-o ControlMaster=auto -o ControlPath=<shared_ssh_control_path> -o ControlPersist=600 -o ServerAliveInterval=15 -o BatchMode=yes`，
  與 herdr **Machines** 共用同一個控制 socket。**不改 upstream**（不公開 `write_managed_ssh_config`）。
- 指令：`herdr desk --ssh <target> [--session <name>]`（spike 專用；實作時改成必填的 `--remote-session`，見結果 §6）。遠端視窗標題加主機前綴。
- root pane：遠端沒有 `root_pane_id`，走 B14 的退回規則（第一個沒有 label 的 pane）——順便驗證這條路。
- **不碰 omarchy 的預設 Session**：所有遠端操作都用獨立的 Session `desk-b22`，量測前啟動、結束後 `server stop`。

## 4. 量測與驗證

| # | 項目 | 方法 | 通過條件 |
|---|---|---|---|
| 1 | 基本操作 | pty 驅動：開啟、輸入、新增視窗、改名、關閉、最小化 | 與本機相同；`herdr tab list`（遠端）看到改名 |
| 2 | 輸入延遲 | desk 記錄「送出按鍵 → 該視窗下一個 frame」的時間（`HERDR_DESK_STATS`），本機與遠端各打 50 個字元 | 回報中位數與 p95；與 `ssh` 直接 echo 的 RTT 對照 |
| 3 | 串流成本 | 1 與 15 個遠端視窗跑 `seq`，同 B13 方法 | 本機 desk 與 ssh 程序 CPU、頻寬（KiB/s）、遠端 server CPU |
| 4 | ControlMaster | 15 視窗時數 TCP 連線（`lsof -i`）與 ssh master 程序 | 到 omarchy 的 TCP 連線只有 1 條 |
| 5 | 斷線 | `ssh -O exit` 關掉 master；以及暫時阻斷網路（`pfctl` 不用；改用 `kill -STOP` master 程序模擬卡住） | 視窗標示斷線、desk 不卡死；恢復後重開串流或可手動重開 |
| 6 | **Attention** | 用一個需要密碼或未知 host key 的目標（`BatchMode=yes` 會失敗） | desk 立即顯示 `! auth` 並結束或停在提示，不會卡住或跳出密碼輸入 |

## 5. 不做

- 合併到 `main`、**Machines Folder**（B15）、多台 **Machine** 同時顯示（B15 決定）。
- Windows 目標（ezpad 沒有 herdr 以外的工具鏈；留給 V2 之後）。
- 遠端 herdr 安裝或升級（沿用 `machine add` 的既有流程）。

## 6. 決策（已核准）

1. **可以在 omarchy 上開一個獨立 Session `desk-b22`（第二個 herdr server，量測時跑 15 個 `seq` 約 1 分鐘、用滿數顆 CPU），結束後停掉嗎？** 建議：可以。預設 Session 不碰。
2. **受管 SSH 選項**：
   - A（建議）：desk 自己組選項並共用 `shared_ssh_control_path`，不改 upstream。
   - B：把 `remote/attach.rs::write_managed_ssh_config` 改成 `pub(crate)` 直接用（會 `Include` 你的 `~/.ssh/config`，與 herdr 行為完全一致；多一個 upstream 改動點）。

## 7. 產出與 commit

| commit | 分支 | 內容 |
|---|---|---|
| `docs: plan b22 remote desk spike` | `main` | 本計畫 |
| `chore: spike herdr desk windows over ssh` | `spike/b22-remote-desk` | spike 程式與量測腳本 |
| `docs: record b22 remote desk spike results` | `main` | `docs/audit/2026-10-25-b22-remote-desk-spike.md`、計畫狀態、BACKLOG（B22 完成、B15 依結論更新） |
