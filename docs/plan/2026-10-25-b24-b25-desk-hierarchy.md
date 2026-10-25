# B24、B25：刪除桌面 Machines 圖示；Window = workspace、Task = Machine

Status: Landed（owner 2026-10-25，見 §6、§7）
Amends（核准後）：ADR 0007 決定 1、2、4，新 ADR 0008。

## 1. 需求（owner 2026-10-25）

1. 桌面上的 `▣ Machines` 圖示意義不大，刪掉。
2. 重新對應階層，對齊 herdr（之前考慮不周）：
   - **Window** 對應 workspace，裡面有 tabs。
   - **Task** 對應 **Machine**（不能改名），裡面有 windows。

## 2. 評估

### B24 刪除圖示：小、低風險
- 程式：`app.rs` 的 `Hit::Icon`、`folder::render_icon` 呼叫與函式。約 −30 行。
- **Machines Folder** 仍可從 Start Menu → Machines 打開，功能不受影響。
- 文件：desk.md Screen 表、glossary **Desktop**（「desktop icons」）。

### B25 階層：合理，而且更接近 herdr
- herdr 自己的 sidebar 就是 `▾ omarchy` → `· api`（Machine → workspace → tab）。新對應和它一致；現在的「Task = workspace、Window = tab」反而把 Machine 這層藏在 `<machine>: <task>` 字首裡。
- **Taskbar** 按鈕從「每個 workspace 一顆」變成「每台 Machine 一顆」（Local、omarchy、smbnfs…），數量少而穩定，`<machine>:` 字首可以拿掉。
- Task 不能改名：名稱就是 Machine 的 label（Local 固定叫 Local）。要改名用 `herdr machine rename`。
- 全部改動都在 sshdr 自有的 `src/cli/desk/`，不碰 upstream 檔案。API 已經有需要的資料：`WorkspaceInfo.active_tab_id`、`tab_count`，`TabInfo.workspace_id`，`SessionSnapshot.layouts`（split 版面）。
- 工作量：中到大（M–L）。`app.rs`（1992 行）的視窗模型要從「一個視窗一條 stream」改成「一個視窗一個 workspace、多個 tab、目前 tab 一條 stream」；加 tab 列的繪製與點擊；`state.rs` 存檔 key 改成 workspace；`taskbar.rs` 的按鈕來源改成 Machine。`server.rs`、`stream.rs`、`transport.rs`、`wm.rs` 大致不變。

### 新的行為對照

| 動作 | 現在 | 改後 |
|---|---|---|
| Taskbar 按鈕 | 每個 workspace | 每台 Machine（Local 永遠在） |
| 點按鈕 | 列出該 workspace 的 tab 視窗 | 列出該 Machine 的 workspace 視窗 |
| 按鈕右鍵 | Rename、New window、Minimize/Restore all、Close task | New window、Minimize/Restore all、Connect/Disconnect（遠端）；沒有 Rename、Close |
| 視窗 | 一個 tab | 一個 workspace，標題下一列是 tab 列 |
| 視窗標題 | tab 名稱 | workspace 名稱 |
| 視窗 Rename | `tab rename` | `workspace rename`；tab 右鍵 Rename → `tab rename` |
| New window | `tab create` | `workspace create` |
| 視窗內新 tab | 無 | tab 列的 `+` → `tab create --workspace` |
| 關閉視窗 | `tab close` | 見決定 3 |
| 存檔 key | terminal id | workspace id（舊 `desk.json` 的視窗位置作廢，視窗重新堆疊一次） |

## 3. 需要注意的地方

- **SSH Tier 的 Machine**：現在是「本機 workspace（以 Machine 命名）裡的 tab 跑 `ssh`」。改後這些本機 workspace 要歸到 smbnfs 的 Task，而不是 Local。用 workspace 的 metadata token 標記（例如 `sshdr.machine=<profile id>`，API 已有 workspace tokens），不靠名稱比對。
- **Native Window**（Machines Folder、之後的檔案管理員）不屬於任何 Machine，見決定 4。
- 同一個 workspace 在 herdr 預設 Client 裡也看得到；tab 的切換若寫回 server（`tab focus`），會影響其他 Client 的畫面，見決定 2。
- 視窗至少要多一列給 tab 列，最小高度從 3 變 4。

## 4. 待決定

1. **視窗內 tab 的顯示**
   - A（建議）只顯示目前 tab 的 root pane（和現在一樣不顯示 split），標題寫 `(+N panes)`。之後要顯示 split 可以另開一項，用 `layouts` 做。
   - B 這次就照 herdr 的 split 版面顯示一個 tab 的所有 pane（每個 pane 一條 stream，複雜度和 stream 數都增加）。
2. **切換 tab**
   - A（建議）只在 desk 裡切，不寫回 server；每個視窗只開目前 tab 的一條 stream，切換時重新接（遠端約 0.3–0.5 秒）。
   - B 同步 `tab focus` 到 server（與 herdr 預設 Client 一致，但會互相影響）。
3. **關閉視窗**
   - A（建議）`[×]` = 關閉 workspace（`workspace close`），有超過一個 tab 時先確認；tab 列上的 `×` 關單一 tab。
   - B `[×]` 只把視窗從桌面收起（像最小化，workspace 繼續跑），要真的關要從右鍵選單。
4. **Native Window 歸屬**
   - A（建議）歸在 Local 的 Task。
   - B 不屬於任何 Task，不出現在 Taskbar。
5. **Taskbar 顯示哪些 Machine**
   - A（建議）Local 加上已連線或已有視窗的 Machine；未連線的 Machine 從 Start → Machines 連。
   - B 所有已存的 Machine 都顯示（未連線的顯示灰色，點了就連線）。

## 5. 分批（舊版，見 §6）

| 批次 | commit |
|---|---|
| B24 | `feat: desk drops the machines desktop icon` |
| B25 | `docs: adr 0008 desk windows are workspaces and tasks are machines`、`feat: desk windows are workspaces with tabs`、`feat: desk tasks are machines`、`docs: record b25 desk hierarchy` |

## 6. Owner 決定（2026-10-25）

1. **Tab 顯示**：照 herdr 的 workspace 版面，上面一列 tab 頁籤，內容是目前 tab 的完整 split 版面（不只 root pane）。
2. **切換 tab**：同步到 server（`tab focus`）。點 pane 也同步 `pane focus`。
3. **`[×]`**：關閉 workspace，超過一個 tab 時先確認；tab 頁籤上的 `×` 關單一 tab。
4. **Native Window**：未定。owner 目前的想法是把「Machine 開出來的 Task」當成一個 App 來收納；之後的 App（檔案管理員等）一起想。這次維持現狀：Machines Folder 不歸任何 Task。
5. **Taskbar**：Local 加已連線的 Machine。

### 技術依據（split 版面）
- `session.snapshot` 的 `layouts[]`（`PaneLayoutSnapshot`）每個 tab 有 `splits[]`（方向、比例）與 `panes[]`（pane id、是否 focused），desk 用比例在視窗內重新算每個 pane 的位置，不用 server 那邊的絕對座標。
- 每個看得到的 pane 開一條 `terminal session control` stream；pane 的 PTY 大小跟著 desk 算出的格子走（和現在一個 tab 一條 stream 的做法相同）。
- 遠端每個 master 最多 8 條 stream 的規則不變，pane 多時會多開 master。

### 修正後的分批

| 批次 | 內容 | commit |
|---|---|---|
| B24 | 刪除桌面圖示 | `feat: desk drops the machines desktop icon` |
| B25 | ADR 0008；Task = Machine；Window = workspace，tab 頁籤（`tab focus` 同步、`+` 新 tab、`×` 關 tab、右鍵 Rename）；`[×]` 關 workspace；存檔 key 改 workspace；這一批 tab 內容先只顯示 root pane | `docs: adr 0008 desk windows are workspaces and tasks are machines`、`feat: desk tasks are machines`、`feat: desk windows are workspaces with tabs` |
| B26 | tab 內照 split 版面顯示所有 pane，pane 間畫分隔線，點 pane 同步 `pane focus`，鍵盤輸入送到 focused pane；這一批不支援拖曳分隔線改比例 | `feat: desk shows split panes inside a tab` |
| 文件 | desk.md、glossary（**Task**、**Terminal Window**、**Desktop**）、CHANGELOG、BACKLOG | `docs: record b24 b25 b26 desk hierarchy` |

## 7. 結果

| commit | 內容 |
|---|---|
| `6b7f421a` | B24：刪除 `Hit::Icon`、`folder::render_icon` |
| `3194389b` | ADR 0008 |
| `dced6625` | B25：`WinData` 改成一個 workspace（tabs、shown tab、`panes`）；Taskbar 按鈕＝`task_keys()`；tab 頁籤 `render_tabs`；`[×]` 關 workspace（多 tab 先確認）；`desk.json` 改用 workspace key，新增 `ssh_workspaces` |
| `c51acb41` | B26：`server::build_snapshot` 讀 `layouts`；`layout_rects` 依比例縮放、留一格分隔線；每個 pane 一條 stream；`DeskEvent::Frame` 帶 serial；點 pane → `pane.focus` |

與計畫不同的地方：
- B25 計畫拆兩個 feat commit（tasks are machines、windows are workspaces），兩者共用同一個資料模型，實作時合成一個 commit。
- **SSH Tier** 的 `ssh` 視窗歸屬記在 `desk.json` 的 `ssh_workspaces`，沒有用 workspace metadata token：herdr 的 token 是「display-only」，會顯示在 herdr 的 UI 上，而且不保證保存。
- 遠端視窗標題保留 `<machine>: ` 字首，不同 Machine 的同名 workspace 才分得出來；Taskbar 按鈕已經不帶字首。
- 每個視窗新增 tab 時，`ssh` 視窗的新 tab 也會跑 `ssh`。
- 點 pane 用 API `pane.focus`：CLI 的 `pane focus` 只能依方向移動。
- 新 tab（`tab create`）與沒有 `layouts` 項目的 tab 先只顯示 root pane，下一個 snapshot 會補上版面。
- 沒有做效能量測（AGENTS.md 的 1 與 15 個 pane 對照）。draw 迴圈每個視窗多一次 `layout_rects`（每個 pane 幾次整數運算）；stream 數量隨 pane 數增加，這是需求本身。

實測（`desk-b15l` 本機、`desk-b15` 遠端，pty 140×40）：

| 情境 | 結果 |
|---|---|
| 啟動 | Taskbar：`Local ·4   omarchy ·1`；每個 workspace 一個視窗，標題下是 tab 頁籤（`1 ×  2 ×  3 ×  +`） |
| 點遠端 `evt-remote` 頁籤 | 內容換成該 tab（`TAB-TWO-MARK`）；遠端 `tab list` 的 focused 跟著變 |
| 從外部 `herdr tab focus w1:t1` | desk 視窗內容換回 `TAB-ONE-MARK` |
| 點 `+` | 遠端多一個 tab `3` 並顯示；點它的 `×` 關掉，tab 回到兩個 |
| tab 右鍵 | Rename tab…、Close tab |
| `^\ n`（焦點在 omarchy 視窗） | 在 omarchy 建 workspace，按鈕變 `omarchy ·2` |
| 兩個 tab 的視窗按 `^\ w` | 問 `Close ~ and its 2 tabs?`，Enter 後遠端 workspace 消失 |
| Machines Folder 雙擊 `smbnfs-only` | 判斷為 SSH，開本機 workspace `smbnfs-only`（`PS C:\Users\Admin>`），Taskbar 多一顆 `smbnfs-only ·1`；重開 desk 後仍歸在該按鈕 |
| `ssh` 視窗點 `+` | 新 tab 也是 `PS C:\Users\Admin>`；關閉視窗先確認，按鈕消失 |
| 本機三格 split（右、左上 0.3） | 視窗內照版面顯示 `PANE-P1`/`PANE-P2`/`PANE-P3` 與分隔線 |
| 點右邊 pane 後打字 | server 的 `focused_pane_id` 變成 `w3:p2`，`echo typed-into-p2` 只出現在右邊 |
| 遠端執行 `pane split` | 約 4 秒內 desk 視窗變成上下兩格，再 split 變三格 |

單元測試：`tab_strip_lists_tabs_with_close_marks_then_new_tab`、`layouts_scale_into_the_window_with_dividers`、`layouts_give_tabs_their_panes_and_zoom_keeps_the_focused_one`、`entries_keyed_by_terminal_id_load_without_a_key`。
