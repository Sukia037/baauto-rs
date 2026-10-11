# 補齊功能實測（2026-10-11）

先前只跑部分核心功能就停止，未完成接收器逐項對照。本輪補上完整測試；**預設設定仍有失敗，不能宣稱所有基本功能都能直接使用**。

## 已實測的結果

| 項目 | 接收器結果 | 證據 |
|---|---|---|
| 技能1–5、技能連點1–5、卡幀開牌1–5、暫停、自動、速度、左鍵連點、快卡幀 | 全部通過；核對送出順序與配對釋放 | [預設時序測試](default-actions.log)、[原始CSV](default/Six-Right-LeftToRight/receiver.csv)、[每項QPC區間與期望序列](default/Six-Right-LeftToRight/cases.json) |
| 慢卡幀（原始時序） | **失敗：預期兩組Esc Down/Up，實際只有第一組** | 同上；[實際keys profile](default/Six-Right-LeftToRight/keys.json) |
| 全域停止、全域錄點通知 | 替代鍵盤組合通過；停止連點後1與左鍵均釋放，錄點計數變動會通知控制程序 | [修正前錄點通知失敗](record-before-fix.log)、[修正後完整測試](controlled-actions.log) |
| 六人右／左各3格、十人左5格、十人右5格反向順序 | 每格輸出正確；各組框外都沒有合成輸出 | `default/` 與 `controlled/` 下各組CSV、cases.json、points.json、hand.json |
| 真正錄點與RButton、XButton1、XButton2、Space、Tab擷取 | 回覆正確；捕捉期間接收器沒有收到被捕捉的Down/Up | [captures.json](controlled/Six-Right-LeftToRight/captures.json)、原始CSV |
| GUI「＋錄製」→假遊戲點擊→新增候選 | 通過：介面顯示候選(906,500)，保持停止；整輪接收器LeftDown/Up共0筆 | [介面截圖](../../../screenshots/windows-20261011-current-dpi/native-recorded-candidate.png)、[UI CSV](ui/ui-receiver-after.csv)、[metadata](ui/ui-receiver-after.json) |
| 校準前景切換 | 原本失敗，修正後通過：不同程序的preview介面在前景時，請求會等到接收器確實在前景才成功 | [修正前](calibration-before-fix.log)、[修正後](controlled-actions.log) |
| 原生校準覆蓋層顯示／60秒逾時 | 覆蓋層實際顯示三格，逾時關閉且維持停止；settings的hand與測試前完全相同 | [覆蓋層截圖](../../../screenshots/windows-20261011-current-dpi/native-calibration-overlay.png)、[測試前settings](ui/settings-before.json)、[測試後settings](ui/settings.json) |
| 既有IPC／輸入標記／持有／停止回歸 | 通過，保持原斷言 | [capture-regression.log](capture-regression.log) |

原始时序的23項逐項對照為22通過、慢卡幀1失敗；這個計數使用替代全域熱鍵，不代表本機原本Ctrl+F9已可註冊。輸入均為測試版本的Windows SendInput注入，經實際Hook、Engine、Driver與接收器，不能當作實體硬體或真實遊戲驗收。

獨立控制組只把`double_esc_second`晚到容許值改成5ms，23項及校準前景切換均通過；實際測試profile保存在`controlled/`，原始profile保存在`default/`。沒有修改正式範本或`C:\BAauto-test`。這不是遊戲時序校準，沒有標記verified。

各接收器正常關閉，metadata均為complete=true、lost_records=0；原始QPC頻率10000000。建置與自動回歸結果見 [automatic.log](automatic.log)。最終正式release已重新建置，不含test-hooks／fault-injection。

源碼提交、環境和原始profile／證據SHA-256見[environment.json](environment.json)。Windows的fmt、clippy（workspace/all-targets/all-features）、cargo test（160通過、3略過）與正式release build均通過；Linux、真實遊戲及長時間測試未在本輪執行。

## 修正和仍失敗的原因

1. `SupervisorStatus`原本只在state、wanted或恢復代號變動時發送；停止狀態按錄點不改這三者，所以GUI收不到records變動。通知條件加入records，原生測試先重現逾時，修正後通過。
2. 校準在`SetForegroundWindow`後立刻呼叫`GetForegroundWindow`，誤把尚未完成的跨輸入佇列啟用當失敗。改成監督迴圈持續處理健康／停止訊息，等前景確認後回覆；使用既有health timeout作控制期限，恢復代號失效、啟用、退出或逾時則拒絕。没有增加阻塞等待、Hook或Engine輸出。[Microsoft對跨佇列非同步啟用的說明](https://devblogs.microsoft.com/oldnewthing/20161118-00/?p=94745)。
3. 慢卡幀把第二組Esc排在`gap_max`，原始晚到容許值0ms。接收器實際只收到第一組；原始碼會在超過最晚期限時取消，5ms獨立控制組可完成兩組，支持排程晚到導致取消的判斷。保留預設失敗與兩組Esc的斷言；真正遊戲的可用上限尚未校準，待決定事項已寫入`docs/decisions.md`。

這輪新測試草稿最初要求連點放開時最後一輪也完整送完，與停止規格要求取消未提交的後續點擊衝突。正式判據要求至少兩輪完整順序，最後只允許已配對釋放的技能鍵前綴，禁止未配對事件／錯序；另保留既有持有與停止斷言。擷取測量則在全域錄點的修飾鍵Up已派送後開始，避免把前一項的在途Up算成捕捉洩漏。既有掛起測試未修改，仍保留失敗。

## 尚未驗收

- 校準四角／四邊拖拉、中心點計算、鏡像確認儲存、Esc取消。桌面工具把覆蓋層拖拉位置判為接收器，啟用並重新觀察後重試仍得到`point (1275, 844) is over baauto-receiver.exe ... not target window baauto-app.exe`；無法確定是透明视窗工具定位限制或實作問題，不能標成通過。這輪只證明顯示、前景切換和逾時。
- 原本Ctrl+F9註冊衝突、既有掛起測試仍失敗，詳見[主報告](../README.md)。
- 實體鍵盤／滑鼠、完整GUI切檔與匯入、OS DPI／最小視窗／多螢幕、8小時接收器、真實遊戲六人／十人各1小時、M5時序校準。

## 重跑

```powershell
$env:BAAUTO_TEST_STOP_HOTKEY = '=Ctrl+Alt+F24'
$env:BAAUTO_TEST_RECORD_HOTKEY = '=Ctrl+Alt+F23'
# 原始時序：本機預期慢卡幀失敗，保留結果。
try {
    cargo build --release --workspace --features test-hooks
    cargo test -p baauto-win-platform --features test-hooks --test windows_actions -- --ignored --nocapture --test-threads=1
    # 獨立控制組；不更改範本，也不能算遊戲校準。
    $env:BAAUTO_TEST_DOUBLE_ESC_LATENESS_MS = '5'
    cargo test -p baauto-win-platform --features test-hooks --test windows_actions -- --ignored --nocapture --test-threads=1
} finally {
    Remove-Item Env:BAAUTO_TEST_DOUBLE_ESC_LATENESS_MS -ErrorAction SilentlyContinue
    cargo build --release --workspace
}
```

一般驗證：`powershell -ExecutionPolicy Bypass -File tests/windows/M6.ps1`。加`-Interactive`會執行原IPC測試及本次逐項測試，任何失敗照常回報，finally仍重建正式版。

## 提交與複查

- `ac23526`：23項、手牌各格、捕捉的原生測試與只存在test-hooks的滑鼠／游標測試API。
- `31e6e5f`：停止狀態下全域錄點通知。
- `afce9ba`：校準前景切換的原生回歸測試，先重現原版失敗。
- `734ec6f`：前景非同步修正，不阻塞監督迴圈。
- 本報告與證據另作docs提交，SHA見完成回報。

建議複查`crates/win-platform/tests/windows_actions.rs`的動作序列、停止尾段與捕捉判據；`supervise.rs`的ViewTarget命令、PendingTargetView取消／期限、records通知；`driver.rs`末端test-hooks輔助API。精確行數見主報告。
