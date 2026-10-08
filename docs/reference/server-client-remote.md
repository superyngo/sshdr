# Server/Client 分工與遠端連線

本文件說明 herdr 目前的程序拓樸、**Server** 與 **Client** 的職責，以及三種遠端用法。內容依據 `main` @ `bbb69a07`（與 Upstream 相同）。使用者面的說明在 `docs/next/website/src/content/docs/` 的 `how-to-work.mdx`、`persistence-remote.mdx`、`connecting-machines.mdx`；本文件補上程式碼層的對照。
程式碼位置以 `file::symbol` 標示。

## 1. 本地模式：三種程序、兩個 socket

```text
herdr（TUI Client）──herdr-client.sock（bincode）──┐
                                                   ├── herdr server（daemon）── PTY ── shell / agents
herdr pane list（CLI，短命）──herdr.sock（JSON）───┘
```

| 程序 | 啟動方式 | 生命週期 |
|---|---|---|
| **Server**（`herdr server`） | 第一次執行 `herdr` 且沒有 server 在聽時自動產生（`server/autodetect.rs::spawn_server_daemon`），以 `setsid` 脫離終端 | 常駐；client 離開後仍保留，`herdr server stop` 才會結束；是所有 pane 程序的父程序 |
| **Client**（`herdr`） | `server/autodetect.rs::auto_detect_launch` → `client/mod.rs::run_client` | 前景 TUI；`ctrl+b q` detach 即結束，不影響 server |
| CLI（`herdr pane …` 等） | `cli.rs::maybe_run` → `api/client.rs::ApiClient` | 送一個請求就結束 |

| Socket | 預設路徑（default session） | 協定 | 使用者 |
|---|---|---|---|
| API socket | `~/.config/herdr/herdr.sock` | 換行分隔的 JSON（`{id, method, params}`），支援 `events.subscribe` 串流（`api/server.rs`） | CLI、plugins、外部腳本 |
| Client socket | `~/.config/herdr/herdr-client.sock` | `[u32 長度][bincode]` 框架（`protocol/wire.rs::write_message`），握手後協商 codec | TUI Client、direct attach |

命名 session 使用 `~/.config/herdr/sessions/<name>/` 下的同名 socket（`session.rs::api_socket_path_for`、`client_socket_path_for`）。在 pane 裡可以看到 `HERDR_SOCKET_PATH`、`HERDR_PANE_ID` 等環境變數，CLI 就是靠這些找到所屬的 server 與 pane。

## 2. 職責分工

| 項目 | 歸屬 | 位置 |
|---|---|---|
| PTY 與子程序生命週期 | Server | `pty/`（`pty/actor/unix.rs::PtyIoActor`） |
| 終端模擬（libghostty-vt）與 scrollback | Server | `pane/terminal.rs` |
| Agent 偵測 | Server | `detect/`、`pane/agent_detection.rs` |
| Workspace/tab/pane 結構、pane 分割版面 | Server | `workspace/`、`layout.rs` |
| Session 持久化與還原 | Server | `persist/` |
| 通知觸發 | Server | `server/notifications.rs` |
| Plugins、custom commands 的執行 | Server（在 server 所在的機器上執行） | `plugin_command.rs` |
| Pane 內容繪製成 cell frame | Server | `ui/tab_surface.rs`，經 `server/render_stream.rs` 送出 |
| 外框 chrome（sidebar、tab bar、overlay、mobile） | Client | `client/shell/` |
| 主題、sidebar 設定、keybinding（預設以本地為準） | Client | `client/shell/config.rs` |
| 終端 raw mode、按鍵與滑鼠解析 | Client | `client/input.rs`、`client/terminal_setup.rs` |
| 剪貼簿（含圖片貼上橋接）、Kitty graphics 合成 | Client | `client/clipboard_images.rs`、`client/shell/graphics.rs` |
| 多 endpoint 連線管理（Machines） | Client | `client/endpoint/` |

一個畫面的資料流：

1. Server 送出兩種資料。`ClientShellSnapshot` 是語意資料（workspaces、tabs、panes、agents、keybindings 等）；`PaneSurfaceFrame`/`PaneSurfacePatch` 是 pane 的 cell 畫面。
2. Client 用 snapshot 畫 chrome，把 pane surface 貼進 pane 區，合成後輸出到終端。
3. 使用者輸入時，client 先攔截 chrome 快捷鍵；其餘按鍵轉成語意事件（`ClientPaneInputEvent`）送給 server，由 server 透過 libghostty 編碼成 VT 序列寫進 PTY（Upstream #5017 起，所有 pane 按鍵編碼都走 libghostty）。
4. 新建 tab、重新命名等操作走 `ClientShellEndpointRequest`，內容是 JSON method，與 API socket 使用同一套 method。

相容性：握手是 `endpoint.hello.v1`/`endpoint.welcome.v1`（`protocol/endpoint.rs`，`ENDPOINT_PROTOCOL_GENERATION = 1`）。Client 與 server 版本不必相同，只要 generation 和 codec 相容即可。選用功能透過 capability 宣告，例如 `surface_interest`、`health_check`、`surface_delta`；缺少某個 capability 只會停用對應的功能。Direct terminal attach 另外使用 `protocol/wire.rs::PROTOCOL_VERSION`（目前是 22）。

多個 client 可以同時接上同一個 server，各自看不同的 tab；看同一個 tab 時，最後操作的 client 決定 pane 尺寸。

## 3. 遠端用法

前提：遠端機器必須安裝 herdr，而且跑著 herdr server。herdr 不支援管理沒有安裝 herdr 的主機。

### 3.1 SSH 進去再執行 `herdr`（類 tmux）

Server 和 client 都在遠端機器上執行，本地只是一個 SSH 終端。這不需要任何特殊機制，手機 SSH 也適用。缺點是 client 在遠端，碰不到本地桌面的剪貼簿。

### 3.2 `herdr --remote <ssh-target>`：本地 UI，遠端 Server

```text
本地 herdr client ─ 本地私有 socket ─ SshStdioBridge ─ ssh -T <target> herdr remote-client-bridge
                                                      ─ 遠端 herdr-client.sock ─ 遠端 herdr server
```

1. 產生暫時的 SSH config：先 `Include` 使用者的 `~/.ssh/config`，再補上 keepalive（`ServerAliveInterval=15`），並設定 `ControlMaster=auto`、`ControlPersist=600`、`-C`（`remote/attach.rs::write_managed_ssh_config`、`shared_ssh_control_path`）。`[remote] manage_ssh_config = false` 可以關閉這個行為。
2. 偵測遠端的 OS/arch（`uname` 或 PowerShell），尋找相容的 herdr 執行檔。找不到時會先詢問使用者，再上傳本地 binary 或下載對應的 release。
3. 確認遠端 server 可用；若需要替換，會詢問是否重啟或做 live handoff（`ensure_remote_server_ready`）。
4. 本地起一個 bridge socket，再啟動一個本地 `herdr client` 子程序接上它。遠端的 `herdr remote-client-bridge`（`remote/host.rs::run_remote_client_bridge`）在需要時啟動遠端 server，然後把 SSH stdio 與遠端 client socket 對接。

結果：pane 和 agent 在遠端執行，UI、主題、keybinding 在本地。Keybinding 預設以本地為準，並清空本地 custom commands，避免本地腳本被套用到遠端 session（`client/shell/config.rs`）。`--remote-keybindings server` 可改用遠端的 keybinding 設定。

### 3.3 Saved Machines：一個視窗同時連 Local 與多台遠端

- `herdr machine add <host>` 把 profile 寫進 `~/.local/share/herdr/client/endpoints.json`（`client/endpoint/catalog.rs::EndpointCatalog`）。Profile 只記 id、label、SSH target、session、enabled，不存任何憑證。
- Client 內的 `EndpointSupervisors`（`client/endpoint/supervisor.rs`）為 Local 與每台啟用的 Machine 各維持一條連線。每條遠端連線都走 §3.2 的 bridge，但只做背景重連：不會跳出問題詢問，也不會安裝或升級遠端的 herdr。
- 健康檢查：閒置 5 秒送 ping，10 秒沒有回應就判定連線失效（`client/endpoint/health.rs`）。重連採指數退避，最長 120 秒；連線穩定 60 秒後才重置退避。
- Surface interest：只有目前選中的 Machine 會傳送 pane 畫面，其他 Machine 只更新 snapshot（workspace、agent 狀態、通知），以節省頻寬與遠端 CPU（`client/endpoint/activation.rs`）。
- Sidebar 會切換到多 endpoint 版本（`client/shell/endpoint_sidebar.rs`），依 Machine 分組。

### 3.4 `herdr --machine <id> <cmd>`：CLI 轉送

這是 API 轉送，不是遠端 shell：本地 CLI 解析參數後，經 `ssh … herdr remote-api-bridge` 把 JSON 請求送到遠端的 `herdr.sock`（`cli/target.rs`、`remote/saved.rs::SavedSshApiBridge`）。只有以 API 實作的子命令可以轉送。遠端 OS 與執行檔路徑會快取在 `~/.local/share/herdr/client/ssh-metadata/<profile-id>.json`。

### 3.5 其他

- SSH agent 轉發：server 在 `<api socket>.agent` 維護一個穩定的 symlink，所有 pane 的 `SSH_AUTH_SOCK` 都指向它。重連後 client 會重新註冊，server 再把 symlink 指到新的轉發 socket（`platform/ssh_agent.rs`、`remote/ssh_agent.rs`）。
- Direct terminal attach（`herdr terminal attach`、`herdr agent attach`、`terminal session observe/control`）會把單一 pane 以 ANSI 串流到目前的終端，不經過 chrome。

## 4. 對 sshdr Fleet 的意涵

依 [ADR 0004](../adr/0004-fleet-registered-machines-with-connect-time-tiers.md)，**Host** 一律經 `machine add` 登錄，連線時判定 **Tier**。**Server Tier** 主機適用 §3 的全部機制；**Fleet Operation** 在所有 Tier 都只走一套 SSH 實作，可以重用下表的 SSH 層：

| 可重用 | 位置 |
|---|---|
| Managed SSH config、ControlMaster 共用、keepalive | `remote/attach.rs::write_managed_ssh_config`、`apply_managed_ssh_options`；`platform::shared_ssh_control_path` |
| SSH 子程序執行、逾時、錯誤分類（auth、host key） | `remote/attach.rs::RemoteSsh`、`wait_with_output_timeout`；`remote.rs::is_remote_auth_error`、`is_remote_host_key_error` |
| 遠端 OS/arch 偵測 | `remote/attach.rs::detect_remote_platform` |
| Profile catalog 的原子寫入、label/id 解析 | `client/endpoint/catalog.rs`、`cli/target.rs::resolve_machine` |

只適用於已安裝 herdr 的機器（不能用在 Host）：`remote-client-bridge`、`remote-api-bridge`、server 生命週期與 handoff、endpoint 握手、pane surface、`server.ssh_agent.register`。

Fleet 的執行邏輯放在哪裡仍是開放決策，記錄在 [`../plan/BACKLOG.md`](../plan/BACKLOG.md) B9。
