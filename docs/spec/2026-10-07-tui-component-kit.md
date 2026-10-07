# TUI Component Kit 需求
Status: Draft — 需求仍在雕琢，尚未核准
Date: 2026-10-07
Source: [`../audit/2026-10-07-tui-components.md`](../audit/2026-10-07-tui-components.md)；Backlog B8

本文件只定需求，不定實作。TUI 整體佈局尚未設計（B8），Fleet runtime 的歸屬也尚未決定（B9），兩者都不在本文件範圍內。

## 1. 背景

稽核發現，herdr client chrome 沒有元件抽象：

- 每個元件的 render、key、mouse 分散在 3–4 個檔案。
- 點擊判定集中在 55 個欄位的 `ShellHitMap`。
- 清單、彈窗定位、文字繪製等基元各自重複實作。

在現況下新增一個 Fleet 或 Backup 畫面，至少要修改 5 個 Upstream 的熱點檔案（`state.rs`、`composition.rs`、`overlay_input.rs`、`mouse.rs`、render 檔）。這些檔案在過去 60 天被 Upstream 修改 17–27 次。

## 2. 目標

- G1：新 UI（Fleet 主機清單、Backup Archive 清單、Fleet Operation 進度等）由可組合的 **Component Kit** 元件組成，不必複製既有 overlay 的程式碼。
- G2：新增一個元件時，對 Upstream 熱點檔案的修改限縮在固定且少量的掛接點。
- G3：元件可以脫離 `ClientShellState` 單獨測試。

## 3. 非目標

- 不重寫 Upstream 既有元件（sidebar、tab bar、13 種 overlay）。這些元件只有在 sshdr 需要修改時才考慮遷移。
- 不處理 server 端的 `src/ui/`（pane/tab surface 繪製）。
- 不設計整體佈局與畫面規劃。
- 不變更 wire 協定。

## 4. 需求

| ID | 需求 | 驗收 |
|---|---|---|
| R1 | **單一模組擁有元件**：同一個元件的 render、點擊區、鍵盤處理、滑鼠處理放在同一個模組 | 任選一個 kit 元件，其行為只需讀單一模組即可理解 |
| R2 | **點擊區在元件內**：元件在 render 時登錄具型別的點擊目標，滑鼠事件由元件自己解讀；不新增 `ShellHitMap` 欄位；不在 render 之外重算幾何 | 新增 kit 元件時 `ShellHitMap` 的欄位數不變 |
| R3 | **元件只回傳意圖**：key/mouse 處理回傳意圖（例如關閉、送出某個 method、無動作），由上層決定是否發 RPC；元件不直接呼叫 `push_endpoint_method` | kit 元件沒有呼叫任何 endpoint/RPC API |
| R4 | **基元**：kit 至少提供文字（寬度計算、寫入、截斷）、面板框線、按鈕、置中彈窗定位、可選取/可捲動/可篩選的清單、scrollbar 轉接（沿用 `crate::ui::scrollbar`） | 每個基元都有 Buffer 斷言的單元測試 |
| R5 | **主題**：顏色一律取自 `Palette`，不寫死色碼；與既有 overlay 視覺一致（AGENTS.md「UI patterns should be reused」） | kit 中沒有寫死的 `Color::Rgb`/`Color::Indexed` |
| R6 | **窄螢幕**：空間不足時元件必須明確降級（回傳 `None` 或改用精簡版面），不可越界寫入；需遵守 mobile 寬度門檻 | 單元測試涵蓋最小尺寸與 0 寬高 |
| R7 | **滑鼠優先、鍵盤對等**：滑鼠可完成的操作，鍵盤也要能完成，反之亦然 | 每個互動元件都有 key 與 mouse 兩組測試 |
| R8 | **效能**：render 中不做 I/O、不鎖 terminal core；每幀配置量與資料筆數成線性（AGENTS.md「Multiplicative performance paths」） | Code review 檢查；清單元件以 1 筆與 1000 筆資料比較 render 成本 |
| R9 | **可測試性**：元件可用純資料加上 `Buffer` 測試；整合測試沿用 `client/shell/tests` 的 `compose()` harness | 不需要 `ClientShellState` 即可完成元件單元測試 |
| R10 | **同步成本上限**：kit 本身全部是 sshdr 新增的檔案；接上 shell 的掛接點以清單列出，每個掛接點只修改一個 match arm 或一個呼叫 | 實作計畫列出所有掛接點；`git diff master..main -- <熱點檔>` 只出現這些掛接 |

## 5. 待雕琢事項

1. 介面形式：採用 ratatui `Widget`/`StatefulWidget`（標準，但 `render` 沒有回傳值，點擊區要另外處理），或自訂 `render(…, hits: &mut HitSink)` 簽名，或兩者並用？
2. 位置：`src/client/shell/kit/`（只給 client 使用）或 `src/ui/kit/`？
3. Overlay 掛接方式：在 `ClientShellOverlay` 增加一個通用的 dynamic 變體，或為 Fleet/Backup 各加具名變體？
4. sshi 的元件：sshi `src/tui/components/`（`input_field`、`member_picker`、`popup`、`viewport`，約 1.5k 行）使用相同版本的 ratatui 0.30 與 crossterm 0.29。要移植、參考，還是不採用？
5. 是否把 ContextMenu 與 GlobalMenu 合併到 kit 作為試點，以驗證設計？代價是會修改 Upstream 檔案。
6. 意圖型別（R3）要用通用 enum，還是每個元件自訂型別再由上層轉換？
