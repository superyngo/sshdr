# B15 Machines Folder 計畫
Status: Approved（owner 2026-10-25：三項都選 A）
Date: 2026-10-25
Source: Backlog B15；[ADR 0004](../adr/0004-fleet-registered-machines-with-connect-time-tiers.md)；[ADR 0007](../adr/0007-desktop-tasks-are-workspaces.md)；[B22 結果](../audit/2026-10-25-b22-remote-desk-spike.md) §5

## 1. 目標

desk 桌面上有 **Machines** 圖示，打開是 **Machines Folder**（**Native Window**），列出 Local 與每台 saved **Machine** 和它的狀態。
連上 **Server Tier** **Machine** 後，它的 **Tasks** 出現在 **Taskbar**，tab 開成 **Terminal Window**，跟本機的一樣操作；
**SSH Tier** 開一個跑 `ssh` 的 **Terminal Window**；需要人工認證的顯示 **Attention**。同時處理 B22 找到的四個問題。

## 2. 前置（已查證）

| 事實 | 出處 |
|---|---|
| saved **Machines** 在 `state_dir()/client/endpoints.json`（`EndpointCatalog::load_profiles`，欄位 id、label、target、session、enabled）；`XDG_STATE_HOME` 可以換掉 `state_dir`，測試用獨立目錄 | `src/client/endpoint/catalog.rs`；`src/config/io.rs` |
| B22 spike 的 SSH 傳輸可用（`spike/b22-remote-desk` 的 `transport.rs`）；`main` 沒有 | B22 結果 §2 |
| sshd `MaxSessions` 預設 10；herdr 自己的 client 也用同一個 master（release 版 namespace 相同時） | B22 結果 §4.4 |
| 測試機：omarchy（Server Tier，0.9.5）；smbnfs（Windows，SSH 可登入、沒有 herdr → SSH Tier）；macmini（host key 未確認 → **Attention**） | 實測 |

## 3. 設計

- **Target**：`Local` 或 `Machine(profile)`。每個 Target 有自己的輪詢執行緒（每秒、**5 秒逾時**），回報 snapshot 或連線狀態（`! auth`、`! offline`）。
- **身分**：**Task** 與 **Window** 都帶 Target；`desk.json` 的 key 改成 `<target>/<terminal_id>`（舊的 key 視為 Local，相容）。遠端 **Task** 在 **Taskbar** 顯示成 `omarchy: api`。
- **斷線不關視窗**：串流結束但 tab 還在時，視窗保留位置，標題標示 `⚠ offline` 並凍結畫面；下一次輪詢成功就在原位重新接上。tab 真的不見才關視窗。
- **串流預算**：每個 master 最多 8 條串流（留給輪詢與 herdr 自己的 client）；超過時用第二個 `ControlPath`（同一路徑加序號）開另一個 master。
- **Tier 判定**（每次連線，不存）：在 **Machine** 上跑 `herdr workspace list`：成功 → **Server Tier**；ssh 回 255 且訊息是認證或 host key → **Attention**；其他 255 → `! offline`；ssh 成功但 herdr 不存在或失敗 → **SSH Tier**。
- **Machines Folder**：桌面左上一個 `Machines` 圖示（雙擊開啟），**Start Menu** 也有一項。清單欄位：名稱、target、狀態（`Local`、`not connected`、`connecting…`、`Server`、`SSH`、`! auth`、`! offline`）。雙擊或 Enter 連線；右鍵選單：Connect、Disconnect。
- **SSH Tier**：在本機建一個以 **Machine** 名稱命名的 **Task**（workspace），新 tab 跑 `ssh <target>`，開成一般 **Terminal Window**。
- 不改 upstream 檔案；`EndpointCatalog` 只讀。

## 4. 步驟與 commit

| # | commit | 內容 | 驗證 |
|---|---|---|---|
| 1 | `feat: desk targets for local and remote machines` | Target、每個 Target 一條輪詢（含逾時）、帶 Target 的身分與 `desk.json` key、遠端 **Task** 標籤 | 單元測試（身分、key 相容、逾時）；pty：Local 行為不變 |
| 2 | `feat: desk keeps windows across stream loss` | 斷線標示與原位重接 | pty：`ssh -O exit` 後視窗位置不變並恢復輸入 |
| 3 | `feat: desk stream budget per ssh master` | 每個 master ≤8 條，溢出開第二個 master | 15 個遠端視窗時，到 omarchy 的 TCP 只來自 desk 的 master（2 條） |
| 4 | `feat: desk machines folder` | 圖示、**Native Window**、清單、Tier 判定、連線與中斷、SSH Tier 視窗、**Attention** | pty：三台測試機各自狀態正確；omarchy 的 **Tasks** 出現並可操作 |
| 5 | `docs: record b15 machines folder` | 計畫結果、`docs/reference/desk.md`、BACKLOG、CHANGELOG | `just ci`、`docs-contract-test`、1 與 15 視窗效能 |

分支 `b15-machines-folder`，完成後合併回 `main`。

## 5. 安全

測試一律用獨立的 `XDG_STATE_HOME`，裡面的 omarchy profile 指向 Session `desk-b15`，**不碰你的 `endpoints.json` 與 omarchy 的預設 Session**。

## 6. 不做

- 圖示檢視（只做清單）、**Fleet Operations**、在 desk 裡新增或刪除 **Machine**（沿用 `herdr machine add/remove`）。
- Windows 版 desk（V2）。

## 7. 待 owner 決定

1. **連到真實 Machine 時用哪個 Session？**
   - A（建議）：用 profile 的 Session（通常是 `default`），desk 看到的就是你在那台機器上的 workspace。注意：desk 用 `--takeover` 接管 pane 大小，同時開著的 herdr client 會看到 pane 大小跟著 desk 變。
   - B：desk 一律用專屬的 Session（例如 `desk`），跟 herdr client 分開，但看不到原本的 workspace。
2. **重開 desk 時要不要自動重連上次連著的 Machine？**
   - A（建議）：要，記在 `desk.json`；連不上就顯示狀態，不跳提示。
   - B：不要，每次從 **Machines Folder** 手動連（ADR 0004 的「需要時才連」最嚴格的解讀）。
3. **SSH Tier 的視窗放在哪個 Task？**
   - A（建議）：每台 **Machine** 一個本機 **Task**，名稱是 **Machine** 名稱。
   - B：全部放在一個叫 `ssh` 的 **Task**。
