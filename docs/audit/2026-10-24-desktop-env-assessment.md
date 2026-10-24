# TUI 桌面環境改造：綜合評估（草稿）
Status: Decided — 見 [ADR 0006](../adr/0006-tui-desktop-as-separate-client.md)
Date: 2026-10-24
Base: `main` @ `4027277f`；Upstream `master` 最後同步 2026-10-24

## 0. 需求（owner，2026-10-24）

暫緩 sshi 整合，改做 TUI 桌面環境：
1. 開始選單與工作列
2. 視窗管理（參考 `~/repos/wens-tui-lab` desk 模式），拖到邊緣自動排列
3. 桌面上的 Machines 圖示，雙擊開啟 Machines 資料夾（類似「我的電腦」），作為所有 Machine 的連線入口
4. 多選、拖曳、剪下/貼上的檔案管理
5. 剪貼簿
6. 依副檔名呼叫指定命令（參考 herdr-sidebar）
7. 桌面成形後，其他功能（包含 sshi）以 App 形式擴充
8. 考量後續 Upstream 同步

## 1. 現況事實

| 項目 | 事實 | 出處 |
|---|---|---|
| Upstream 活躍度 | 90 天 672 commits；`src/client` 144、`src/app` 136、`src/server` 99、`src/client/shell` 72、`src/ui` 42、`src/protocol` 23、`src/layout.rs` 3 | `git log --since=2026-07-24 master` |
| 熱點檔 | `client/shell/state.rs` 29、`server/headless.rs` 51、`client/mod.rs` 49、`app/state.rs` 29 | 同上 |
| 畫面幾何 | Server 決定 pane rect 與 PTY 尺寸（BSP `layout.rs::TileLayout`）；Client 只把 server 渲染好的整張 `PaneSurfaceFrame` 貼上 | `protocol/wire.rs::PaneSurfaceFrame`（「one server-rendered active-tab surface」） |
| 同時可見內容 | 每個 Client 只收「選中 Endpoint 的 active tab」一張 surface；不能同時顯示多個 tab 或多台 Machine 的 pane | `client/endpoint/activation.rs`；`docs/reference/server-client-remote.md` §3.3 |
| 浮動 pane | 只有單一 popup pane（`app/popup.rs`、`ClientShellPopupSurface`） | — |
| 單一終端串流 | `herdr terminal session control <target> --cols --rows`：每個 terminal 一條 client-socket 連線，JSON 控制指令輸入、ANSI 輸出，連線者控制尺寸；無平台限制（只有互動式 `herdr terminal attach` 限 Unix：`platform::capabilities().direct_terminal_attach`），Windows 未實測 | `client/terminal_sessions.rs`、`server/clients.rs::ClientConnectionMode::TerminalAttach` |
| 協定穩定性 | Terminal attach 走私有 same-install 協定（`PROTOCOL_VERSION`），不屬凍結的 generation-1 endpoint 契約 | AGENTS.md「Stable client endpoint contract」 |
| Plugin | 原生程序 + JSON socket API；UI 只能是 pane 內的 TUI（placement: overlay/popup/split/tab/zoomed），不能畫 chrome | `api/schema/plugins.rs`、`app/api/plugins/` |
| herdr-sidebar | 只有本地檔案瀏覽；**沒有**副檔名→命令對照表，只有一個全域 editor 命令與寫死的預覽副檔名集合 | `~/.config/herdr/plugins/github-installations/herdr-sidebar-*/…/actions.rs`、`viewer.rs` |
| 遠端檔案 | herdr 沒有 sftp/遠端列目錄；scp 只用於安裝 binary | `remote/attach.rs` |
| 剪貼簿 | OSC 52 寫出 + 平台原生讀取（`platform::read_clipboard_text/image`），無剪貼簿 crate | `selection.rs`、`client/clipboard_forwarding.rs` |
| wens-tui-lab | ratatui 0.30（與 herdr 相同）；`desk.rs`（410 行，只依賴 `Rect`）、`hitmap.rs`、`menu.rs`、`geom.rs` 可直接搬；`desk_input.rs` 需抽掉 `App/Target`。無桌面圖示、雙擊計時、多選、跨視窗拖放 | `~/repos/wens-tui-lab/src/` |

## 2. 核心結論

需求 2（多個可重疊視窗）與 herdr 現在的顯示模型直接衝突：herdr 一個 Client 只看得到一個 tab 的一張 server 渲染畫面。所以這不是「在 herdr 介面上加功能」，而是**另一個 Client**。

## 3. 架構選項

| | A. 改造既有 ClientShell | B. 獨立桌面 Client（建議） | C. 桌面做成 herdr plugin |
|---|---|---|---|
| 做法 | Tab 新增 float layout（server），ClientShell 加桌面模式 | 新 `herdr desk` 子命令與 sshdr 自有模組；每個視窗 = 本地 Server 上專屬 workspace 裡的一個 pane，經 terminal session 串流顯示 | 一個全螢幕 pane 裡的 TUI |
| 跨 Machine 視窗並排 | 不行（surface 只給選中 endpoint） | 行（遠端內容以本地 pane 的 `ssh`/巢狀 client 呈現） | 不行（無法嵌入其他 pane） |
| 動到 Upstream 熱點 | 多：`layout.rs`、`workspace/`、`ui/tab_surface.rs`、server render、`client/shell/{state,mouse,composition,overlay_input}.rs`、protocol | 少：CLI 分派 1 處；其餘新檔 | 無 |
| 同步成本 | 高，且持續 | 低 | 無 |
| 持久化（detach 後視窗仍在） | 有 | 有（pane 在本地 Server）；視窗幾何存 sshdr 自有檔案 | 有 |
| 主要風險 | 衝突、需新 codec | 需在 client 端把 ANSI 解析成 cell grid（ghostty-vt 已在 repo）；私有協定隨 Upstream 變；Windows 未實測 | 做不出真視窗 |

B 也符合 Upstream 方向：「server-owned runtime protocol with the TUI as one client」。桌面只是另一個 client。

### B 的視窗種類

- **Native 視窗**：桌面自己畫（Machines 資料夾、檔案管理、設定、開始選單）。可參與跨視窗拖放與檔案剪貼簿。
- **Terminal 視窗**：本地 Server 的 pane（shell、`ssh host`、巢狀 `herdr --remote`、App）。是 PTY 內的 TUI，**無法**參與桌面拖放，除非之後提供桌面 API（socket/CLI）。

所以需求 4（拖放、剪下貼上）必須是 Native，不能做成 App。

## 4. 逐項評估

| # | 需求 | 可行性 | 重點與風險 | 規模 |
|---|---|---|---|---|
| 1 | 開始選單 + 工作列 | 高 | lab 已有 taskbar 與 `Menu<A>`；開始選單內容 = App 清單 + Machines + 設定 | S |
| 2 | 視窗管理 + 邊緣吸附 | 高（B） | 搬 `desk.rs` 吸附/格線/tile/cascade；新增：視窗尺寸變更 → terminal 串流 resize；15 視窗效能量測（AGENTS.md multiplicative paths） | M |
| 3 | Machines 資料夾 | 高 | 讀 `client/endpoint/catalog.rs::EndpointCatalog`（唯讀）；雙擊：Server Tier → 開 Terminal 視窗跑 `herdr --machine/--remote`（巢狀，需處理 prefix 鍵衝突）；SSH Tier → `ssh host`。狀態/Tier 偵測與 B11/B12 重疊 | S–M |
| 4 | 檔案管理（多選、拖放、剪下貼上） | 中 | **最大項**。本地用 `std::fs`；遠端需新 SSH 檔案層（ControlMaster 上的 sftp 或 `ssh` + 列目錄指令；Windows 主機要 PowerShell 分支）；跨 Machine 複製 = scp/rsync，等於 sshi `cp/sync` 的子集 → 與「暫緩 sshi」有張力。另需：衝突對話框、進度、取消、大目錄、symlink、權限 | L |
| 5 | 剪貼簿 | 中 | 兩種：文字（沿用 OSC 52 寫出 + 平台讀取；在 SSH 進去的遠端桌面讀不到本機剪貼簿）與檔案剪貼簿（桌面內部，記 Machine + 路徑 + cut/copy） | S–M |
| 6 | 副檔名 → 命令 | 高 | herdr-sidebar 沒有可抄的對照表，需自訂：`ext → argv 模板（{path}）`、開啟位置（新視窗/背景）、在檔案所在 Machine 執行。可參考 herdr `[[link_handlers]]` 的 pattern 寫法 | S |
| 7 | App 擴充 | 高 | 建議 App = herdr plugin manifest 的超集（沿用 `[[panes]]`/`[[actions]]`，加桌面欄位：圖示、開始選單分類、副檔名關聯、`placement = "window"`），Upstream 的 plugin 改進可直接受益 | M |
| 8 | Upstream 同步 | — | B 方案下掛接點只有 CLI 分派；仍要追蹤 terminal session 私有協定變動。B3（hard-fork 時機）可延後 | — |

## 5. 建議分期

| 期 | 內容 | 驗證 |
|---|---|---|
| P0 Spike | 2–3 個本地 pane 以 terminal session 串流 + client 端 VT 解析，畫成兩個可拖曳、可 resize 的視窗；確認 resize、鍵盤/滑鼠轉送、detach 後重連 | 真 binary 在 pty 驅動（沿用 lab `scripts/pty_drive.py`）；1 vs 15 視窗效能 |
| P1 桌面殼 | 視窗、工作列、開始選單、吸附、視窗幾何持久化 | Buffer + hit map 測試；pty 驅動 |
| P2 Machines 資料夾 | 圖示、清單/圖示檢視、狀態、雙擊連線 | 真 Machine 連線 |
| P3 檔案管理（本地） | 多選、拖放、剪下/複製/貼上、檔案剪貼簿、副檔名關聯 | 單元 + pty |
| P4 檔案管理（遠端） | SSH 檔案層、跨 Machine 複製 | 真遠端（含 Windows 主機與否待定） |
| P5 App 模型 | manifest 擴充、開始選單註冊、桌面 API | 範例 App |

## 6. 對既有文件的影響（決策後處理）

- 新 ADR：產品方向改為桌面環境、選擇方案 B；`glossary.md` 的 **sshdr** 定義需改。
- BACKLOG：B1/B2 暫緩（移到 Watching 或標註）；B8 的 Component Kit 改成桌面自有元件（R10 掛接點限制在 B 方案下大致自動滿足）；B9 在 B 方案下的答案傾向 Client/本地 Server。
- 新 glossary 詞彙（提案）：**Desktop**、**Window**（Native / Terminal）、**Taskbar**、**Start Menu**、**Machines Folder**、**App**、**File Association**、**File Clipboard**、**Snap**。

## 7. 待 owner 決策

1. 架構：B（獨立桌面 Client）/ A / 其他？
2. 桌面是否取代 `herdr` 預設入口，或先以 `herdr desk` 子命令並存？
3. 遠端檔案管理是否列入第一版？若列入，SSH 檔案層等同先做 sshi `cp` 的一部分。
4. 目標平台：Windows 本機 client 與觸控/窄螢幕平台是否在第一版範圍？
5. 遠端 Server Tier 主機的 pane 以巢狀 herdr client 呈現可接受嗎？（雙層畫面、prefix 鍵衝突）
6. Hard-fork（B3）是否與此方向綁定？B 方案下可延後。

## 8. 決策（owner，2026-10-24）

1. 採 B。
2. 以 `herdr desk` 子命令並存，不取代預設入口。
3. 遠端檔案管理列入第一版（P4）；SSH 檔案層先做 sshi `cp` 的子集。
4. 第一版支援 macOS、Linux；Windows 本機列入範圍，P0 實測不通過再降級。觸控平台（不限 Termux：
   點擊 = 按下＋放開，沒有拖曳、右鍵、長按，lab contracts C12）延到第二期，屆時另做觸控版。
5. 接受以巢狀 herdr client 呈現 Server Tier 主機。
6. Hard-fork 不與此方向綁定；B3 維持延後。
