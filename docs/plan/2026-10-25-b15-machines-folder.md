# B15 Machines Folder 計畫
Status: Landed（owner 2026-10-25：三項都選 A）
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

## 8. 結果

| 步驟 | commit | 驗證 |
|---|---|---|
| 1–3 | `d2067604 feat: desk targets for local and remote machines` | 見下；步驟 2、3 與 1 一起提交（同一批檔案），commit 內文已註明 |
| 4 | `286a19f2 feat: desk machines folder` | 見下 |

測試環境：獨立 `XDG_STATE_HOME=/var/tmp/desk-b15/state`（只含三筆測試 profile），本機 Session `desk-b15l`，
omarchy 的 profile 指向 Session `desk-b15`。腳本與輸出在 `docs/tmp/claude-scratch/b15/`（`desk_drive.py` 加了依畫面文字點擊）。

| 情境 | 結果 |
|---|---|
| `desk.json` 記著 omarchy，重開 desk | 自動重連；**Taskbar** 出現 `omarchy: api ·1`，視窗標題 `omarchy: 1`，可輸入 |
| 關掉 desk 的 master（`ssh -O exit`） | 視窗留在拖過去的位置，約 1 秒內原位重接，可繼續輸入 |
| 暫停 master（`kill -STOP`） | 約 5 秒後提示列顯示 `omarchy ! offline`、標題變 `⚠ omarchy: 1`；`CONT` 後恢復 |
| 15 個遠端視窗 | desk 到 omarchy 只有 2 條 TCP（master 0 與 master 1）；第一次實作時 7 條串流同時搶建 master 1，6 條變直連，改為先建 master 再開串流後消失 |
| **Machines Folder** | 雙擊圖示開啟；macmini（host key 未確認）→ `! auth`；smbnfs（Windows、無 herdr）→ `SSH`，在本機 **Task** `smbnfs` 開出 `PS C:\Users\Admin>` 視窗；omarchy 右鍵 Disconnect → **Task** 與視窗消失；雙擊重連 → 視窗回到存下的位置 |

效能（release，200×60，每視窗 `seq`，20 秒，`desk_perf.py`；同一台機器先後跑 `main` 與本分支）：

| 視窗數 | desk CPU main → B15 | 繪製平均 main → B15 | frame/s main → B15 |
|---|---|---|---|
| 1 | 2.8% → 2.8% | 0.287 → 0.286 ms | 54 → 54 |
| 15 | 11.8% → 12.1% | 2.90 → 2.98 ms | 300 → 296 |

沒有可量測的差異。這次 15 視窗的 server CPU 只到約 230%（B14 時約 440%），因為測試時機器負載高（load average 約 40），兩邊條件相同。

### 測試中的失誤

- 第一次斷線測試用迴圈對 `/tmp/hssh-1000/` 下所有 socket 送 `ssh -O exit`，連帶關掉了 herdr client 自己連 omarchy 的 master（`41c18f…`）。
  herdr client 在 1 分鐘內自動重建（14:50 新 socket），之後的測試只對 desk 的 socket 下指令。

### 留下的問題

- `machine add` 還不會存 **SSH Tier** profile（B11），所以 smbnfs 這類主機目前只有手改 catalog 才會出現。
- 遠端 **Machine** 上有 herdr 但沒有在跑的 server 時，會被判成 **SSH Tier**（`herdr workspace list` 失敗）；desk 不會替遠端啟動 server。
- 連上的 **Machine** 每秒 3 個 ssh 子程序輪詢（B23 的 `events.subscribe` 也適用遠端）。
