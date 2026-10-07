# TUI 元件清點與模組化評估
Status: Draft — 等待決策（見 §7）
Date: 2026-10-07
Base: `main` @ `bbb69a07`（TUI 程式碼與 Upstream `master` 相同，fork 尚未分歧）

## 1. 範圍與方法

靜態閱讀，沒有 build 或執行。範圍包括 `src/ui/`、`src/ui.rs`、`src/client/shell/`、`src/client/shell.rs`，以及這些程式碼依賴的 `src/protocol/wire.rs::ClientShellSnapshot`、`src/app/state.rs::Palette`、`src/popup_size.rs`。
三個 scout 分別清點 chrome、overlays、架構，原始輸出在 `agent://ChromeScout`、`agent://OverlayScout`、`agent://ArchScout`（session 結束即失效，重點已併入本文）。§8 列出經人工抽查過的數字與說法。

## 2. 架構總覽：兩條繪製路徑

```text
Server (src/server/headless/render.rs)
  ├─ ClientShellSnapshot（語意資料，wire codec shell.snapshot.v1，凍結契約）──► control stream
  └─ crate::ui::render_tab_surface（pane 內容 + pane chrome，虛擬 Buffer）
        └─ PaneSurfaceFrame / Patch ─────────────────────────────────────► render stream
Client (src/client/shell_runtime.rs → ClientShellState)
  input.rs / mouse.rs / overlay_input.rs ─► 改寫 ClientShellState（91 個欄位）
  composition.rs::compose()
    ├─ render.rs::render_shell() → sidebar / tab bar / mobile header，回傳 ShellHitMap
    ├─ 貼上 PaneSurfaceFrame
    ├─ overlays / notifications / link hover
    └─ ComposedFrame → frame_output.rs（diff → ANSI → stdout）
```

- `src/ui/` **不是** client chrome。它是 server 端的 pane/tab surface 繪製（`panes.rs`、`tab_surface.rs`），加上一批 client 也會重用的**純函式**：scrollbar 計算、sidebar row/token 模型、release notes 行模型、`truncate_end`、`centered_popup_rect`、`modal_stack_areas`。
- Client chrome（sidebar、tabs、overlays、mobile）全部放在 `src/client/shell/`。
- 舊的 server 端全螢幕 TUI 只剩 `TerminalAttach`/`TerminalObserve` 的 ANSI 路徑還在用，client shell 一律走 `SemanticFrame`。
- 整個 codebase 沒有任何 `impl Widget`/`StatefulWidget`。每個元件都是 `fn(&mut Buffer, Rect, …, &mut ShellHitMap)` 形式的自由函式。

## 3. 元件清冊

### 3.1 常駐 chrome（client）

| 元件 | 位置 | 讀取 | 寫入 hit map | 備註 |
|---|---|---|---|---|
| 版面計算 | `state.rs::ClientShellConfig::layout` → `ClientShellLayout` | config、cols/rows | — | 固定 4 區：`sidebar`、`tab_bar`、`mobile_header`、`pane_surface`；寬度 ≤ `mobile_width_threshold` 時切換 mobile 版面 |
| Sidebar（單一 endpoint，展開） | `sidebar.rs::render_sidebar` | snapshot、config、`ShellRenderState` | workspaces、dividers、scrollbar、toggle、new_workspace | 上方 workspace 樹，下方 agent panel |
| Sidebar（收合） | `sidebar.rs::render_collapsed_sidebar` | snapshot、config | workspaces、agents、toggle | 寬度 4 寫死 |
| Sidebar（多 endpoint） | `endpoint_sidebar.rs::render_expanded` / `render_collapsed` | 同上 + endpoints、diagnostics、collapse maps | machines、endpoint_agents… | 與 `sidebar.rs` 結構高度重複 |
| Agent panel | `agent_sidebar.rs::render_agent_panel` | agent entries、sort、scroll | agents、agent_body、scrollbar、sort toggle | 重用 `crate::ui::sidebar_agent_rows`/`resolved_token_spans`；自帶一份 `put_text`/`display_width` |
| Tab bar | `tabs.rs::render_tab_bar` | snapshot、tab_scroll、drag index | tabs、new_tab、scroll L/R | `MIN_TAB_WIDTH`、`NEW_TAB_WIDTH` 寫死；拖放落點由 `mouse.rs` 另行計算 |
| Mode bar | `render.rs::render_mode_bar` | mode、copy mode、error、keybinds、palette | 無（回傳 `Option<Rect>`） | 用鍵盤操作 |
| Mobile header / navigation | `mobile.rs::render_mobile_header` / `render_mobile_navigation` | snapshot、config、scroll | mobile_switch/close/targets | 自己實作一套可捲動清單 |
| Pane 合成 | `composition.rs` + `crate::ui::render_selection_highlight` | `PaneSurfaceFrame`、palette | panes、popup、pane_splits | Kitty graphics 遮蔽處理也在這裡 |

### 3.2 Overlay（`state.rs::ClientShellOverlay`，13 個變體，同時只會有一個）

| 變體 | Render | Key 輸入 | 滑鼠 | 檔案數 |
|---|---|---|---|---|
| Onboarding | `overlays.rs` + `ui/onboarding.rs` 常數 | `overlay_input.rs` | `mouse.rs` | 4 |
| ProductAnnouncement | `overlays.rs` + `ui/release_notes.rs` | `overlay_input.rs` | `mouse.rs` | 4 |
| ReleaseNotes | 同上 | `overlay_input.rs`（`current_release_notes_input_geometry` 會再算一次幾何） | `mouse.rs` | 4 |
| Rename | `overlays.rs` | `overlay_input.rs` | `mouse.rs` | 3 |
| ConfirmClose | `overlays.rs` | `overlay_input.rs` | `mouse.rs` | 3 |
| Help | `overlays.rs` | `overlay_input.rs` | `mouse.rs` | 3 |
| Navigator | `overlays.rs` + `aggregate_navigation.rs` | `overlay_input.rs` | `mouse.rs` | 4 |
| WorktreeCreate / Open / Remove | `worktree_overlays.rs` | `worktrees.rs` | `mouse.rs` | 4 |
| ContextMenu | `overlays.rs` + `context_menu.rs` | `overlay_input.rs` | `mouse.rs` | 4 |
| GlobalMenu | `overlays.rs` + `global_menu.rs` | `overlay_input.rs` | `mouse.rs` | 4 |
| Settings | `settings_overlay.rs` | `settings.rs` | `mouse.rs` | 3 |

不在 enum 內的浮層：`notifications.rs::render_visible_notification`（toast）、`endpoint_notices.rs::render_notice`（重用 `render_notification_card`）、`link_hover.rs::render_link_hover`（tooltip）、`crate::ui::render_config_diagnostic_buffer`、`render_copy_feedback_buffer`。

結論：沒有任何 overlay 把 render、key、mouse 放在同一個模組。

### 3.3 共用基元（現況）

| 用途 | 現有實作 | 重複/分歧 |
|---|---|---|
| 文字寬度/寫字 | `ui/text.rs::display_width`（usize）、`truncate_end`；`render.rs::put_text`/`put_segment`/`put_right_text`/`display_width`（u16） | `agent_sidebar.rs` 另有一份 `put_text` 與 `display_width` |
| 面板框線 | `overlays.rs::panel`（逐格畫 `┌─┐`） | 沒用 ratatui `Block`；sidebar 和 pane 的分隔線也是各自逐格畫 |
| 按鈕 | `overlays.rs::button`；`ui/widgets.rs::close_button_rect`/`continue_button_rect` | worktree、settings overlay 各自手刻按鈕 rect |
| 置中彈窗 rect | `ui/widgets.rs::centered_popup_rect`；`overlays.rs::popup`/`popup_with_width_cap`；`popup_size.rs::resolve_popup_geometry`（terminal popup 用） | 三份，邊距規則不同 |
| Modal 分區 | `ui/widgets.rs::modal_stack_areas` | 只有 release notes、announcement 和另一個 modal 使用，其他 overlay 手刻 |
| Scrollbar | `ui/scrollbar.rs`（thumb、drag 換算、`render_scrollbar_buffer`）；`scroll.rs::render_list_scrollbar` | 基元本身有共用；`overlays.rs` 有一處直接手算 `scrollbar_thumb` |
| 可捲動/可篩選清單 | 無共用元件 | Navigator、Help、WorktreeOpen、Settings、ContextMenu、GlobalMenu、mobile navigation 各自實作選取、捲動、row 命中 |
| 對比色 | `ui/widgets.rs::panel_contrast_fg` | `overlays.rs::contrast` 是同一套邏輯的重複 |
| 主題 | `app::state::Palette`，以 `&Palette`/`config.palette` 傳遞 | 一致性尚可 |

## 4. 模組化問題（依對後續擴充的影響排序）

1. **元件三分裂**：render、key、mouse 分散在 `overlays.rs`(1317 行)、`overlay_input.rs`(1163)、`mouse.rs`(2401)。`ClientShellOverlay::` 在 `overlay_input.rs` 被 match 58 次，`mouse.rs` 37 次。新增一個 overlay 至少要動 `state.rs`（enum、`kind()`、hit map 欄位）、`composition.rs`、render 檔、`overlay_input.rs`、`mouse.rs` 五處。
2. **集中式 `ShellHitMap`**：55 個具名欄位，每個元件的內部幾何都要暴露給中央 `mouse.rs`。另外，拖放落點（`tab_drop_index_at`、`workspace_drop_target_at`）和 release notes 輸入幾何會在 render 之外另算一次，兩邊必須手動保持一致。
3. **沒有元件抽象**：沒有 `Widget`、沒有 overlay trait、沒有統一的 render context。`render_shell` 依賴的 `ShellRenderState` 帶 18 個欄位，其中多個是 `&mut usize`/`&mut bool`，表示 render 過程中會回寫捲動與 reveal 狀態。子面板無法單獨拿出來重用。
4. **缺清單元件，基元也重複**：至少 6 處自己做「選取 + 捲動 + 搜尋 + row 命中」，popup rect 有 3 套，`display_width`/`put_text` 有 2–3 套。Fleet host 清單、Backup archive 清單都會是這種清單，沒有共用元件就只能複製貼上。
5. **版面固定**：`ClientShellLayout` 只有 4 個槽位，`render_shell` 用 `if` 鏈依序畫出。沒有 dock/panel 槽位，也沒有「整個工作區切換成另一種 view」的機制。
6. **Sidebar 雙軌**：`sidebar.rs` 和 `endpoint_sidebar.rs` 依 endpoint 數量分支，大量重複。若 Fleet 在 sidebar 加入 Host 分組，會變成第三份。
7. **UI 與動作耦合**：overlay 的 key handler 直接改寫 `ClientShellState` 並呼叫 `push_endpoint_method`（例如 `save_rename_overlay`、`submit_worktree_create`）。沒有「UI 回傳意圖、由上層執行」這一層。
8. **可見性與 import**：子模組普遍 `use super::*`，`render.rs` 用 `#[path = "../shell/…"]` 引入 `overlays`/`sidebar`/`tabs`。可見性多半是 `pub(super)`，元件搬家成本高。

做得好、應保留的部分：render 不碰 I/O；`compose()` 測試 harness（`tests/mod.rs` 的 `snapshot()`/`surface()`/`frame_rows()`）已能斷言 cell 內容與 hit map，可作為 characterization test 的基礎；scrollbar、token row 等純函式已經有共用。

## 5. 新增一個元件需要跨越的邊界

| # | 邊界 | 位置 | Fleet/Backup 是否必需 |
|---|---|---|---|
| 1 | 資料來源 | server `AppState`，或 client 本地（類似 `EndpointCatalog`） | 取決於 B1/B6；Fleet 直接 SSH 到 Host，資料很可能屬於 client 本地 |
| 2 | Wire 欄位 | `ClientShellSnapshot`（`shell.snapshot.v1` 已凍結） | 只有在資料來自 server 時才需要；依規則必須是 optional 欄位或新 codec |
| 3 | Client 狀態 | `ClientShellState` / `ClientShellOverlay` | 是 |
| 4 | 版面槽位 | `ClientShellLayout` + `render_shell` | 是（若是常駐面板） |
| 5 | Render | `client/shell/*.rs` | 是 |
| 6 | Hit map | `ShellHitMap` 新欄位 | 是（現況下） |
| 7 | 輸入分派 | `input.rs`、`overlay_input.rs`、`mouse.rs`、`actions.rs` | 是 |
| 8 | 測試 | `client/shell/tests/` | 是 |

## 6. 強化方向（建議，尚未核准）

**關鍵限制：Upstream 改動頻率。** 過去 60 天 Upstream `master` 共 461 個 commit，其中 77 個動到 `src/client/shell` 或 `src/ui`；`state.rs` 被改 27 次，`mouse.rs`、`input.rs`、`composition.rs` 各 17 次。在 fork 裡大規模重構這些熱點檔案，之後每次同步 Mirror Branch 都要付出衝突成本。因此建議採**加法優先**：新元件建在新模組上，既有元件只在必要時遷移。

| 階段 | 內容 | 動到 Upstream 熱點？ | 驗證 |
|---|---|---|---|
| P0 | 為要遷移或掛接的區域補 characterization tests（`compose()` + `frame_rows()` + hit map 斷言） | 只新增測試檔 | `just test` |
| P1 | 新增 sshdr 自有的元件 kit（例如 `src/client/shell/kit/`）：`text`（統一 width/put/truncate）、`panel`（框線 + 清背景）、`button`、`popup_rect`（統一置中規則）、`ListView` 狀態 + render（選取、捲動、篩選、row 命中），以及 `HitSink`（`Vec<(Rect, Target)>` 泛型命中登錄） | 否，純新增 | kit 單元測試（Buffer 斷言） |
| P2 | 定義 `ShellPanel`/`ShellOverlay` trait：`render(&self, ctx, area, buf) -> Hits`、`on_key`/`on_mouse -> Outcome`。`Outcome` 只回傳意圖（`Close`、`Dispatch(Method)`、`None`），由上層執行 RPC。`ClientShellOverlay` 加一個 `Dynamic(Box<dyn ShellOverlay>)` 變體，`composition`/`overlay_input`/`mouse` 各只加一個轉送分支 | 小（3 處各 1 個 match arm） | 新 overlay 透過 harness 測試 render、key、mouse |
| P3 | 版面擴充：`ClientShellLayout` 加 optional `dock` 槽位，或新增「主區 view 模式」（pane surface ↔ Fleet view） | 中（`state.rs` layout、`render_shell`） | layout 單元測試；mobile 寬度門檻回歸 |
| P4（選擇性） | 把 Upstream 既有元件遷到 kit：先合併 ContextMenu 與 GlobalMenu，再處理 sidebar 雙軌 | 高 | 只在要修改這些元件時才做；或以 PR 回饋 Upstream（需符合 External contributor guardrail，本 fork owner 不在 APPROVED_CONTRIBUTORS 時不可送 PR） |

是否採用 ratatui `Widget`：ratatui 的 `Widget::render` 沒有回傳值，無法帶出 hit region。建議 kit 元件使用自訂簽名 `render(self, area, buf, hits: &mut HitSink)`，純裝飾性元件再另外實作 `Widget`。

## 7. 待決策事項

1. Fleet/Backup 的 UI 形態：overlay（類似 Navigator 的彈窗）、常駐 dock 面板，還是主區 view 模式？這會決定 P3 是否必要。
2. Fleet 資料歸屬：client 本地（不經 wire）或 server 狀態（要擴充 `ClientShellSnapshot` 或新增 API method）？與 B1/B6 綁在一起。
3. 改造策略：加法優先（建議）或直接重構 Upstream 元件？後者會持續增加同步衝突。
4. Kit 放在 `src/client/shell/kit/`（client 專用）或 `src/ui/kit/`（server 也可用）？目前 chrome 只在 client 繪製，建議前者。

## 8. 查核紀錄

已人工抽查：
- `ShellHitMap` 有 55 個欄位，`ClientShellState` 有 91 個欄位，`ClientShellOverlay` 有 13 個變體。
- `ShellRenderState` 有 18 個欄位。
- 全 codebase 沒有 `impl Widget for`。
- `overlays.rs` 確實有 `panel`、`popup`、`popup_with_width_cap`、`button`、`contrast`。
- `modal_stack_areas` 共 4 處呼叫（overlays.rs 3 處、overlay_input.rs 1 處）。
- `agent_sidebar.rs` 有自己的 `put_text`/`display_width`。
- Client 端有重用 `crate::ui` 的 scrollbar、sidebar token、release notes、`truncate_end`。

修正 scout 的說法：ArchScout 稱 `src/ui` 與 client「不共用」，與事實不符，client 有部分共用（見上）；OverlayScout 稱 Help/Navigator「內嵌 scrollbar 繪製」，實際上它們呼叫的是共用的 `render_scrollbar_buffer`，只有一處手算 thumb。
未查核：scout 對各元件「寫死尺寸」的細節，例如 Onboarding 的 72x18。
