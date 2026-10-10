# M2 實作與驗證

已接上 Hook → 固定 SPSC → Engine/Driver → SendInput 的 Windows 路徑。共享記憶體包含固定版面標頭、三類進度、保守輸出帳本及 8192 筆原子紀錄環；Control 可在停止後倒出 CSV。停止與放開批次優先，外部注入預設不觸發，自家標記直接放行。普通點位與牌鍵仍沿用 M1 的動作邏輯。

## 驗收對應

- Hook 規則、左右修飾鍵、Exact chord 失效、焦點後 Up 配對、溢位後禁止新動作：`crates/win-platform/tests/acceptance.rs`。
- 負原點、多螢幕、邊界與 50000 像素寬桌面精確落點；十萬次 Hook 操作零配置。
- 三個 Loom 模型、慢讀取者正確計算遺失、並行讀取不接受撕裂紀錄、短送帳本保留：`crates/protocol/tests/acceptance.rs`。
- Windows Event、Timer、Mapping 測試：`windows_impl/kernel.rs`；互動桌面測試：`tests/windows_interactive.rs`。
- 六人右側／十人左側接收器 fixtures、分開的預設按鍵 JSON、可調大小的接收器及 Alt+F4 正常倒出 CSV。
- native Hook 初始化確認 TIME_CRITICAL；其他執行緒維持預設優先權。

## 已執行

`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`，以及 `cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings`。Mac 主機的 portable tests 與 Windows 全目標交叉編譯通過。真實 Windows API 與桌面測試未執行。

## Windows 待驗收

1. `cargo test -p baauto-win-platform` 跑 Event／Timer／Mapping 原生測試。
2. `powershell -ExecutionPolicy Bypass -File tests/windows/M2.ps1 -Mode six`；再用 `-Mode ten`。接收器先啟動，引擎等待目標前景後 Arm。
3. 每個停止／失焦情境重新開始 standalone session；停止後不自動重新啟用。互動測試可用 `cargo test -p baauto-win-platform --test windows_interactive -- --ignored --nocapture --test-threads=1`。
4. 仍需人工確認全部手牌格、外側無輸出、失焦 LeftUp、Down/Up 配對、溢位、程序被殺時帳本、五分鐘 CPU 滿載與 1000 次實體觸發延遲。尚無實機 P50/P99/max 數據；不以模擬值替代。

## 獨立複查

M2-review 完成唯讀獨立複查。修正：初始實體鍵未同步、實體先按輸出鍵、左右 VK alias、游標查詢失敗漏 Up、DoubleEsc forced cancel、Card 第一個 Esc in-flight 放開、Engine 初始化錯誤未通知 done、Ctrl+C 已觸發 Event 空轉、WinEvent 安裝失敗／子物件誤判、接收器 Alt+F4 與極寬桌面落點。對應回歸測試已補。

## 與草案差異／待決定

遵守規格的兩種 JSON，`--config` 只作點位檔別名，使用 `--keys` 與 `--settings`。紀錄環各欄位統一 AtomicU64，增加設定、幾何和來源序號；使用 SeqCst 協定較草案 Release/Acquire 更強。Progress 增加固定實體鍵位圖以涵蓋雙向 PhysicalConflict。鍵盤先用 wVk＋wScan，不開 SCANCODE；待實機比較。上述事項已列入 `docs/decisions.md`。

## 建議複查

- `crates/win-platform/src/hook.rs`：press 配對、實體鍵、修飾鍵與輸出衝突。
- `crates/win-platform/src/windows_impl/runtime.rs`：callback、Hook 啟動、Engine 等待與 Control 停止清理。
- `crates/win-platform/src/windows_impl/driver.rs`：帳本先記後送與單一輸出執行緒。
- `crates/protocol/src/shm.rs`：原子紀錄環與帳本快照。

提交：`6df57fc`（測試與介面）、`7c697cb`（實作），第三筆為本回報及新增回歸／Windows 驗收測試；完整 SHA 以 Git 歷史為準。
