# M6 互動接收器與首頁警告（2026-10-12）

環境：本機 Windows MSVC、Rust / Cargo 1.99.0，接收器螢幕 1920×1080、QPC frequency 10000000。來源程式與 `4272d3a` 相同；後續 `18f5792`、`9d82953` 只修改文件。沒有啟動真實遊戲。

## 指令與結果

執行 `tests/windows/M6.ps1 -Interactive`：

- [預設熱鍵輪](default-hotkeys.log)：等待 Armed 逾時，IPC 測試失敗，尚未執行慢卡幀；finally 已重建正式版。
- [隔離替代熱鍵輪](alternate-hotkeys.log)：只設定 `BAAUTO_TEST_STOP_HOTKEY='=Ctrl+Alt+F24'`、`BAAUTO_TEST_RECORD_HOTKEY='=Ctrl+Alt+F23'`；整個腳本 exit code 0。fmt、workspace/all-targets/all-features Clippy、自動測試（163 passed、3 ignored）、release build、兩項 ignored 互動測試均通過，最後再次重建不含 test-hooks 的正式版。
- 慢卡幀 **PASS**：時序維持正式範本 `escape_gap_ms=[16.0,50.0]`、`key_hold_ms=16.0`，實際收到兩組 Esc Down／Up；沒有增加晚到容許值或修改等待。快卡幀亦通過。
- 預設動作、普通點位持有／釋放、六人左右三格／十人左五格及右五格反向、框外不送、全域停止／錄點、右鍵／側鍵／Space／Tab 擷取與校準前景交接均 PASS。四組接收器 metadata 均 `complete=true`、`lost_records=0`。
- 正式版首頁：用新的暫存 data-dir 啟動，實際看到 Ctrl+F9、Ctrl+F10 被佔用的紅色提示，「啟用」停用，且有「前往「預設按鍵」改鍵」連結。沒有使用 preview 或 `--qa-hotkey-warning`；桌面工具已檢視畫面，未保存截圖。檢查後已正常關閉。

## 慢卡幀原始證據

message 256=Down、257=Up；wparam 27=Esc，四筆均帶同一自家標記：

| 原始 QPC | message | wparam | extra_info |
|---|---|---|---|
| 14271428350462 | 256 | 27 | 1936071385 |
| 14271428556773 | 257 | 27 | 1936071385 |
| 14271429054557 | 256 | 27 | 1936071385 |
| 14271429218000 | 257 | 27 | 1936071385 |

原始 [CSV](Six-Right-LeftToRight/receiver.csv)、[案例 QPC 區間](Six-Right-LeftToRight/cases.json)、[按鍵／時序 profile](Six-Right-LeftToRight/keys.json)、[metadata](Six-Right-LeftToRight/receiver.json)，以及其他三種模式的 profiles／cases／captures 均保存在本目錄。輸入經 test-hooks 的 Hook／Engine／Driver 與真實 Windows 接收器，不代表實體硬體或遊戲效果驗收。

## 尚未驗收

改鍵儲存後首頁警告解除的完整 GUI 操作、校準拖拉／確認儲存、OS DPI／混合螢幕、故障注入、真實舊檔／遊戲，以及 [自動測試報告](../README.md) 的三項 ignored 尚未補。首頁檢查只證明警告呈現與啟用停用，不將整組熱鍵／GUI 驗收勾成通過。沒有新增程式／規格差異、依賴或待決定事項。
