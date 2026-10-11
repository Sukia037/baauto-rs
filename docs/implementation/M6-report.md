# M6 實作回報

M6 的桌面介面、檔案流程、校準覆蓋層、錄點／熱鍵擷取與背景紀錄已實作。可攜式測試和 Windows GNU 交叉檢查通過；目前執行環境是 macOS，Windows 原生功能、真實 DPI、遊戲與長時間測試尚未驗收。不能把本回報解讀成遊戲內成功率或延遲保證。

## 對應任務驗收

| M6 項目 | 實作與證據 | 驗收狀態 |
|---|---|---|
| GUI 純狀態邏輯 | 選取、衝突排序、存檔條件、獨立兩種 JSON、停止／套用／恢復流程；校準框四角、四邊、鏡像與中心推算 | Linux 可跑的測試已在 macOS 通過；CI 設置 Linux 執行 |
| 三欄介面、中文、淺深色 | 四分頁、兩列檔案、鍵帽、詳細編輯、底部草稿與狀態；內嵌 Noto Sans TC | 24 張實際 macOS GUI 截圖；Windows 100／150／200% DPI 待驗收 |
| 檔案管理 | 內容辨識、拖入、任意路徑、分頁、遺失重新指定、另存、新增、重新載入、`.trash`；M4 editor/store、外部變更指紋與原子存檔 | 背景工作整合測試通過；網路磁碟／OneDrive／Windows 長路徑待驗收 |
| 錄點／熱鍵擷取 IPC | Disarmed 限制、CaptureReady、15 秒 QPC 逾時、取消序號、session／request id 過濾、成對吞掉 Down／Up、Armed 拒絕 | 純邏輯與協定測試通過；Windows ignored 整合測試已交叉檢查，尚未執行 |
| 手牌校準 | 透明置頂 viewport、拖角／邊、兩中心計算、左右預覽／鏡像、60 秒逾時、Esc、寫入機器設定；負座標螢幕定位 | 計算測試通過；混合 DPI／穿透／遊戲對位待驗收 |
| 停止與非阻塞 GUI | GUI 只傳命令；監督、檔案、紀錄各有背景執行緒；原始恢復代號防止停止後舊操作重新 Arm；關閉立即停止 | 可攜回歸測試通過；真實負載／焦點／關閉待驗收 |
| 功能對照與長測 | 23 個功能、六／十人、匯入、停止及 8 小時接收器／各 1 小時遊戲的操作清單 | [acceptance-m6.md](../acceptance-m6.md) 全部原生項目保持未勾選 |
| M5 優先權／自旋 | EngineParams 維持 Hook TIME_CRITICAL、Engine NORMAL、spin=0 | 缺少原生 A/B 證據，尚不能定案新的預設 |

兩種 JSON 的草稿互相獨立；底部儲存會預先驗證兩者。若多檔儲存中只有一個成功，訊息列出已提交的檔，更新其外部指紋並保持停止，不宣稱跨檔交易原子性。時間不合法的草稿保留供修正，不能另存繞過驗證。

每次擷取先完成停止，Hook 模式就緒才顯示可以輸入。完成擷取後保持停止，需要明確啟用。一般檔案操作／手牌校準可保留原啟用意願，但整串操作使用最初的恢復代號；任何手動／全域停止均使它失效。關閉請求立即停止，即使使用者最後取消關閉，也保持停止。

接收器改為固定 8192 筆 SPSC 佇列和背景 CSV 寫入，顯示僅留最近 20 筆；不再因一百萬筆記憶體容量截斷八小時資料。正常關閉寫出 `complete: true`、遺失數和 QPC 頻率；磁碟錯誤、佇列遺失或強制結束保留部分資料，不能算完整長測。app 觀測共享 ring 並持續保存原始 QPC，GUI 通道 256 筆、畫面紀錄最多 1000 筆。

## 驗證指令與結果

使用 Rust stable 1.99.0、edition 2024；依賴鎖定於 Cargo.lock。以下為本機實際執行的檢查：

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo clippy --workspace --all-targets --all-features --target x86_64-pc-windows-gnu -- -D warnings
cargo build -p baauto-app
```

全部通過；workspace 共 154 個可執行測試，零失敗。Windows 交叉 Clippy 包含一般版與實驗／test-hooks，確認 Windows 分支的型別與警告，但沒有連結或執行 Windows 程式。GUI 以原生 macOS OpenGL framebuffer 截圖事件產生四分頁 × 淺深色 × 100／150／200% 應用程式 zoom；[截圖索引與雜湊](../screenshots/README.md) 記錄其限制。

Windows 上執行 `pwsh -File tests/windows/M6.ps1`；加 `-Interactive` 跑需前景接收器的 IPC ignored 測試，含基準 Space 事件、擷取後事件不進接收器、逾時／取消／Armed 拒絕及停止後舊代號不能恢復。腳本最後重建不含 test-hooks 的正式二進位。加 `-Screenshots` 取得目前真實 Windows DPI 下的八張圖，需在 OS 設定 100／150／200% 及混合 DPI 後分別重跑並保留輸出。完整操作見驗收清單。

尚未執行：Windows 系統字型、原生對話框與檔案系統故障、全域 RegisterHotKey、透明／穿透覆蓋層、真實側鍵與錄點、接收器全部功能、8 小時負載、六／十人各 1 小時非排名遊戲、PresentMon／DXGI／M5 已驗證校準與優先權 A/B；也尚無使用者真實舊檔可覆核。

## 差異、依賴與待決定事項

- 全域「緊急停止／錄點」依架構使用 RegisterHotKey，限定鍵盤組合。其餘 21 個預設功能和點位接受右／中／側鍵；每格按鍵僅接受單一鍵盤鍵。這是通用熱鍵要求的已知限制，沒有另加全域滑鼠 Hook 或把 Raw Input 健康證據變成觸發器。
- 長路徑保留完整字串，可水平捲動及滑過查看；無法在 960px 視窗同時展開無限長字串。截圖的 zoom 不等於 Windows OS DPI，較大視窗會受 macOS 桌面限制。
- 沒有 Windows A/B 數據，保持規格預設，不宣稱 M5 性能門檻通過。範本時間仍未驗證，不製造 verified 校準結果。
- 新增 rfd 0.17.2 做原生檔案對話框、image 0.25.10（只有 PNG）保存 GUI 截圖；eframe／egui 0.36.2 是任務指定類型的依賴。原生依賴樹沒有 tokio runtime；Cargo.lock 的 tokio 是 UI 依賴宣告的 WASM 傳遞項。內嵌字型附 SIL OFL 與[來源記錄](../../bins/baauto-app/assets/README.md)。
- [decisions.md 的 M6 節](../decisions.md) 已記錄：15／60 秒 GUI 逾時是否可配置、擷取與取消關閉保持停止、原始恢復代號、全域鍵盤限制、有限 AtLeast 超集註冊、字型與 DPI、截圖來源、紀錄遺失及接收器串流。這些暫定選擇不等於使用者已核准新的產品語意。

## 建議複查

行號以本次程式提交為準，後續編輯請以函式名定位：

| 檔案 | 範圍／重點 |
|---|---|
| `bins/baauto-app/src/backend.rs` | 93–231 Controller、445–825 工作佇列與檔案停止／套用／原始代號 |
| `bins/baauto-app/src/gui.rs` | 205–306 命令／擷取；588–925 檔案列和狀態；1532–1643 手牌；1768–2074 關閉及覆蓋層 |
| `bins/baauto-app/src/state.rs` | 1–214 檔案狀態、衝突、框的純計算 |
| `bins/baauto-app/src/telemetry.rs` | 17–135 共享 ring 觀測、背景 CSV 與丟失證據 |
| `crates/win-platform/src/hook.rs` | 86–125 擷取狀態；219 起成對吞鍵／取消／熱路徑 |
| `crates/win-platform/src/windows_impl/managed.rs` | 330–425 擷取安裝、就緒及取消；相關按鍵放開門檻 |
| `crates/win-platform/src/windows_impl/supervise.rs` | 267–446 恢復代號、停止 barrier、擷取管理 |
| `crates/win-platform/src/windows_impl/health_window.rs` | 288–325 全域 AtLeast／Exact 註冊及完整失敗處理 |
| `crates/win-platform/src/windows_impl/receiver.rs` | 180–337 視窗生命週期、串流與正常關閉證據 |
| `crates/win-platform/tests/windows_capture.rs` | 原生整合驗收；不能因交叉檢查通過就算已執行 |

## 提交

`4e2b039`：純狀態／擷取測試契約；`dc88c21`：GUI、覆蓋層、擷取及恢復管理；`c2fee65`：停止／檔案／全域熱鍵／長測紀錄的實質修正；`bfead8a`：截圖模式隔離歷史視窗尺寸；`4e3fe50`：選取功能說明與時間用途，見[總表](status.md)。交付文件與實際截圖另外保留一筆提交，不 squash。
