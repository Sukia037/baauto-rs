# Windows 自動測試結果（2026-10-12）

測試來源提交：`4272d3ad4a3212020fd3d6b6e846ffd4748189de`。環境為本機 Windows、`x86_64-pc-windows-msvc`、Rust / Cargo 1.99.0、edition 2024。第一輪只執行自動檢查，沒有啟動遊戲或互動接收器；以下結果依本次終端輸出整理，未另外保存原始 log。

| 指令 | 結果 |
|---|---|
| `cargo fmt --check` | 通過，exit code 0 |
| `cargo clippy --all-targets -- -D warnings` | 通過，exit code 0 |
| `cargo test` | 通過，163 passed、0 failed、3 ignored，exit code 0 |

163 項通過測試包含 app 背景工作／狀態、bench 分析、config 驗證／相容／存檔、core 動作／校準、protocol 帳本／IPC／監督，以及 Windows Event／timer／共享記憶體、Hook 判定、擷取純邏輯、全域熱鍵註冊、注入標記及原生 Unicode／長路徑／唯讀存檔測試。晚到 tap／第二個 Esc、舊晚到欄位相容與全域停止鍵佔用後重新綁定的自動回歸均通過。

## 略過與未執行

| 測試 | 略過原因／手動執行方式 |
|---|---|
| `receiver_dxgi_change_has_raw_qpc_and_exportable_roi` | 需要全螢幕前景接收器與實體按鍵。`cargo test -p baauto-bench --test windows_capture -- --ignored --nocapture --test-threads=1` |
| `foreground_hook_focus_loss_and_paired_up` | 需要實體前景／焦點操作。`cargo test -p baauto-win-platform --test windows_interactive -- --ignored --nocapture --test-threads=1` |
| `paths_unc_native` | 需要真實可寫 UNC 共用資料夾，先設定 `BAAUTO_TEST_UNC_ROOT`，再執行 `cargo test -p baauto-win-platform --test windows_store paths_unc_native -- --ignored` |

第一輪沒有啟用 `test-hooks`、`fault-injection` 或 `bench-experiments`；其限定測試未執行，不能算入上述 3 個 ignored。沒有執行 all-features 檢查、release build、GUI／DPI、校準拖拉／儲存、故障注入或遊戲功能驗收。後續同日已完成互動重跑，正式範本慢卡幀收到兩組完整 Esc；詳見 [互動報告與原始證據](interactive/README.md)。前述未執行範圍只描述第一輪自動檢查。

本次無程式／規格差異、未新增依賴或待決定事項。超過 20 分鐘的測試依既有使用者決定不做；其他原生項目的狀態見 [M6 驗收清單](../../acceptance-m6.md)，先前原生結果見 [2026-10-11 報告](../windows-20261011/README.md)。
