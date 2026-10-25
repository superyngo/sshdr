# B23、B21、B11 Desktop 後續計畫
Status: Approved（owner 2026-10-25：四項都選 A）

Owner 2026-10-25 說「都做」，指 B15 報告最後列的三項。順序：B23 → B21 → B11，每項一個分支，各自合回 `main`。

## 1. 目標

| 項目 | 驗收 |
|---|---|
| B23 事件取代輪詢、即時存檔 | 閒置的 desk 不再開 CLI 子程序；直接殺掉 desk 也保留最後的版面 |
| B21 可移動的 **Taskbar** | 上下左右四邊都能放，可以拖拉調整高度或寬度，重開後保留；每個邊都有 hit-map 測試，並用 pty 實際操作過 |
| B11 存下 **SSH Tier** 機器 | `machine add` 到沒有 herdr 的主機時可以存檔，`machine list` 和 **Machines Folder** 都看得到 |

## 2. 前置（已查證）

- `events.subscribe` 在本機和 omarchy（herdr 0.9.5）都能用：一條連線送出訂閱後，`tab_created` 之類的事件會一直送進來。
- 遠端不需要裝新版 herdr：`ssh -W <遠端 socket 路徑> <host>` 會把 stdio 接到遠端的 unix socket，實測可以收到 `subscription_started` 和 `tab_created`。
- 遠端 socket 路徑可以從 `herdr status server --json` 的 `socket` 欄位拿到，0.9.5 也有這個欄位。
- `herdr api snapshot` 在 0.9.5 也有，一次就拿到 workspaces、tabs、panes，可以取代現在的三個 list 指令。
- 查證時的失誤：測試用的 `tab create` 下到了 omarchy 的**預設 session**，建出 `wB:t3`，已經立刻 `tab close`。現在的 tab 和之前一樣是 `w9:t7, wA:t1, wA:t3, wB:t1`。之後所有遠端測試只用 `desk-b15` 這類測試 session。
- `EndpointCatalog` 有 `deny_unknown_fields`。如果在共用的 catalog 加欄位，舊版 herdr 會讀不了整個檔案；herdr client 也會對 catalog 裡每一台機器做 heartbeat。
- `machine add` 遇到「沒有 herdr、拒絕安裝、server 起不來」這些情況時，現在都是回報「machine was not saved」然後結束（`src/cli/machine.rs` `add`）。

## 3. 設計

### B23
- **快照**：本機直接用 API socket 送 `session.snapshot`，不開子程序；遠端用 `herdr api snapshot`，一次 ssh 取代原本三次。
- **事件**：
  - 每個 target 開一條 `events.subscribe` 連線，訂閱 workspace 的 created、renamed、closed、updated，tab 的 created、closed、renamed、moved，pane 的 created、closed。
  - 本機直接連 socket，遠端走 `ssh -W <socket>`，經過 desk 自己的 ControlMaster。
  - 收到事件後等 100 ms，把同一批事件合併，再抓一次快照。
- **斷線**：
  - 事件連線一斷就標成 `! offline`，每 2 秒重試一次；連回來時先抓一次快照。
  - desk 自己的 master 加上 `ServerAliveInterval=3`、`ServerAliveCountMax=2`，所以遠端卡住時，約 6 秒內連線會斷，不需要再靠輪詢偵測。
- **存檔**：
  - 版面一有變動（移動、調整大小、最大化、最小化、疊放順序、連線或中斷機器）就標記為 dirty。
  - 變動停止 1 秒後寫入 `desk.json`，寫法是先寫暫存檔再改名。
  - 退出時照舊存檔。

### B21
- `desk.json` 新增 `taskbar: {edge: bottom|top|left|right, size}`，舊檔案沒有這個欄位時，預設在下方、高度 1。
- **上下兩邊**：
  - `size` 是高度，1 到 4 列。
  - 按鈕放不下時換到下一列；第一列是 Start 和提示文字。
- **左右兩邊**：
  - `size` 是寬度，12 到 40 欄。
  - Start 在最上面，下面每列一個 **Task** 按鈕，提示文字放在最底下並截斷。
- **移動**：在 **Taskbar** 的空白處或 Start 上按住拖曳，放開時看游標離哪一邊最近就放到那一邊。拖曳時畫出目標位置的外框。
- **調整大小**：拖曳 **Taskbar** 朝向桌面那一側的邊。
- **選單位置**：Start 選單、**Task** 選單和提示都依 **Taskbar** 所在的邊決定往哪個方向打開。
- **連帶調整**：桌面範圍、平鋪、貼邊和最大化都改用新的桌面範圍；**Taskbar** 換邊或改大小時，已經最大化的視窗會跟著調整。

### B11
- **SSH Tier** 機器另外存一個 sshdr 自己的檔案：`<client 目錄>/ssh-machines.json`，格式是 `{version, machines:[{id,label,target}]}`。
  - 共用的 catalog 完全不改，所以 herdr client 和舊版 herdr 都不受影響。
- `machine add`：
  - 遇到 SSH 連線或認證失敗：照舊不存。
  - 遇到其他失敗（沒有 herdr、拒絕安裝、server 起不來）：
    - 在互動式終端機會問 `Save as SSH-only machine? [y/N]`。
    - 不是互動式終端機時，只在加了 `--ssh-only` 時才存。
  - 直接加 `--ssh-only` 會跳過安裝，只測一次 SSH 連線就存檔。
- `machine list`、`remove`、`rename` 也會處理這個檔案；`list` 多一欄 tier。
- **Machines Folder** 一起列出這些機器，雙擊直接開 `ssh` 視窗。

## 4. 步驟與 commit

| 分支 | commit |
|---|---|
| `b23-desk-events` | `feat: desk follows server events instead of polling` |
| | `feat: desk saves layout on change` |
| | `docs: record b23 desk events` |
| `b21-desk-taskbar` | `feat: desk taskbar on any edge with adjustable size` |
| | `docs: record b21 movable taskbar` |
| `b11-ssh-tier-machines` | `feat: machine add saves ssh-only machines` |
| | `feat: desk lists ssh-only machines` |
| | `docs: record b11 ssh-only machines` |

每項完成後都會：跑 clippy、跑 desk 的測試、在 `desk-b15` 和 `desk-b15l` 上做 pty 實測、跑完整的 `just ci` 和 `docs-contract-test`，然後 fast-forward 合回 `main` 並 push。B23 另外比較改動前後 1 個和 15 個視窗的效能。

## 5. 安全

- 遠端測試只用測試 session，不在 omarchy 的預設 session 下任何會改東西的指令。
- 只關 desk 自己的 ControlMaster socket，用明確的 hash 指定，不跑整個目錄的迴圈。
- B11 的測試在 `XDG_STATE_HOME=/var/tmp/desk-b15/state` 底下進行，不碰真正的 catalog。

## 6. 待 owner 決定

1. **B23 遠端事件**：
   - A（建議）：用 `ssh -W` 接遠端 socket 收事件，跟本機同一套做法。
   - B：只有本機用事件，遠端維持每秒輪詢，但改成一次 `api snapshot`。
2. **B21 怎麼換邊**：
   - A（建議）：拖曳加右鍵選單都做。右鍵選單有 Position：上、下、左、右。
   - B：只做拖曳。
3. **B11 存在哪**：
   - A（建議）：另存 `ssh-machines.json`。
   - B：寫進共用 catalog，加 tier 欄位。這會讓舊版 herdr 讀不了 catalog，herdr client 也會一直嘗試連這些機器。
4. **B11 什麼時候存**：
   - A（建議）：失敗時在互動式終端機詢問，另外加 `--ssh-only` 旗標。
   - B：只有加 `--ssh-only` 才存。
