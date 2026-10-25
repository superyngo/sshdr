# B24、B25：刪除桌面 Machines 圖示；Window = workspace、Task = Machine

Status: Draft（評估，等 owner 決定 §4）
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

## 5. 分批（核准後）

| 批次 | commit |
|---|---|
| B24 | `feat: desk drops the machines desktop icon` |
| B25 | `docs: adr 0008 desk windows are workspaces and tasks are machines`、`feat: desk windows are workspaces with tabs`、`feat: desk tasks are machines`、`docs: record b25 desk hierarchy` |
