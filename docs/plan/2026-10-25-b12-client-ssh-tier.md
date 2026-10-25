# B12：herdr Client 區分 Attention 與 SSH Tier

Status: Approved（owner 2026-10-25：三項都選 A）
Backlog: B12（P1, M）。依據：ADR 0004 決定 2、4。

## 1. 現況

- `src/client/endpoint/supervisor.rs`：背景連線失敗時，`failure_needs_attention()`（呼叫 `src/remote/saved.rs::saved_ssh_failure_needs_attention`）決定狀態是 `Attention` 還是 `Reconnecting`。
- `Attention` 涵蓋兩種本質不同的情況：
  - 要人處理：`permission denied`、`host key verification failed`、host key 變更。
  - 主機沒有可用的 herdr：`unsupported remote platform`（如 smbnfs 的 Windows X86）、`matching Herdr is not ready … install or update`、`remote server is not ready for saved machines`、`predates Herdr's stable endpoint protocol`。
- 這兩種都在 sidebar 顯示紅色 `! error`，而且每 30 秒重試一次。第二種會永遠重試、永遠紅色。
- desk 已經自己分類（B15 `transport::failure_link`），herdr client 還沒有。

## 2. 目標

- 第二種情況標為 **SSH Tier**：sidebar 顯示 `ssh`（非紅色），不在背景重試（ADR 0004 決定 4：SSH Tier 不保持連線）。
- 第一種維持 `Attention`。
- 網路類失敗維持 `Reconnecting` 加退避。
- 使用者手動 retry、catalog 重新載入、client 重開時，會重新判斷 Tier（ADR 0004 決定 2：Tier 不存檔）。

## 3. 變更範圍（全是 upstream 檔案）

1. `src/remote/saved.rs`：新增 `saved_ssh_failure_is_ssh_tier(&io::Error) -> bool`，判斷上面的四種訊息。`saved_ssh_failure_needs_attention` 不動。
2. `src/client/endpoint.rs`：狀態表示，見決定 1。
3. `src/client/endpoint/supervisor.rs`：失敗時先判斷 SSH Tier；`record_status` 對 SSH Tier 設 `next_attempt = None`（見決定 2）。
4. 顯示：`endpoints.rs::endpoint_status_presentation`、`endpoint_sidebar.rs`、`mobile.rs`、`machine_diagnostics.rs`。
5. 測試：supervisor 分類與重試排程、sidebar 字樣。
6. 文件：`docs/reference/server-client-remote.md`、desk.md 的 B12 註記、CHANGELOG.sshdr.md、BACKLOG。

## 4. 待決定

### 決定 1：狀態怎麼表示
- **A（建議）** `ClientEndpointStatus` 新增 `SshTier`。語意清楚；這個 enum 只在 client 內部，不在 wire 上（不影響 endpoint 相容性規則）。代價：約 8 個 upstream 檔案的 `match` 要加一個分支，upstream 同步時這些地方可能衝突。
- **B** 不加 variant，維持 `Attention`，另外在 `ClientShellEndpoint` 加一個 sshdr 欄位 `ssh_tier: bool` 只影響顯示與重試。改動檔案較少，但 `Attention` 的語意被分成兩種，容易漏改。

### 決定 2：SSH Tier 的重試
- **A（建議）** 不在背景重試；只有手動 retry、catalog 重新載入、client 重開才重新連。符合 ADR 0004 決定 4。缺點：遠端裝好 herdr 後要手動 retry 才會變成 Server。
- **B** 低頻重試（例如 10 分鐘一次）。

### 決定 3：sidebar 字樣
- **A（建議）** `○ ssh`，灰色（overlay 色），選到時的說明列提示 `SSH only: run herdr --remote <host> to install herdr`；不支援的平台（Windows 等）只顯示 `SSH only`。
- **B** 只顯示 `ssh`，不加說明。

`machine add --ssh-only` 存的主機（B11）仍然不給 herdr client 連線，這次不改。

## 5. 驗證

- 單元測試：分類函式（四種 SSH Tier 訊息、auth、host key、網路錯誤），supervisor 對 SSH Tier 不排下一次重試、手動 retry 會重新連。
- 實測（不碰 omarchy 預設 session）：用獨立 `XDG_STATE_HOME` 的 debug client，catalog 有 omarchy(desk-b15)、smbnfs、macmini：
  - smbnfs → `○ ssh`，等 60 秒確認沒有重試（看 log 與 ssh 子程序）。
  - macmini → `! auth`（不變）。
  - omarchy → online（不變）。
  - 對 smbnfs 手動 retry → 重新連一次後仍是 ssh。
- `just ci`、`docs-contract-test`。

## 6. Commits（預先擬定）

| commit | 內容 |
|---|---|
| `feat: client shows ssh tier machines without retrying` | §3 1–5 |
| `docs: record b12 client ssh tier` | §3 6 與本文件結果 |
