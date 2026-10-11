# Windows 實測：2026-10-11

這輪是原生 Windows 的短程驗證，**不是 M6 全面驗收通過**。基準提交為 `1968d10efec974ef4bf3ee313c41189db813c441`；本輪另外修正合成輸入標記與接收器。環境和測試設定 SHA-256 見 [environment.json](environment.json)。

後續已補齊23項接收器對照：替代全域熱鍵下22項通過，原始時序的慢卡幀失敗；獨立5ms晚到容許值控制組23項通過。六人／十人各格、左右／反向順序、框外、真正錄點、側鍵／右鍵／Space／Tab擷取也完成；並修正停止時錄點通知、校準前景切換。詳見[補測報告與原始證據](expanded/README.md)。這些補測仍不代表真實遊戲或長時間驗收。

## 結果

| 項目 | 結果 | 證據 |
|---|---|---|
| fmt、clippy（workspace/all-targets/all-features）、cargo test、production release build | 通過；160 個自動測試，3 個略過 | [automatic.log](automatic.log) |
| 原生 IPC：Armed 拒絕擷取、Point 逾時、Hotkey 取消、Space Down/Up 攔截、舊 generation 不得 Arm | 通過 | [native.log](native.log) |
| 實際 Hook → Engine → SendInput → 接收器：w→1、按住2連點、z普通點位按住/放開 | 通過；合成1和左鍵均有配對釋放，無重複Down，最終無持有 | [receiver.csv](capture-and-actions/receiver.csv) |
| 運作中送註冊的全域停止熱鍵 | 通過；合作釋放1與左鍵，保持停止，舊 generation 不得復活 | [native.log](native.log) |
| 鍵盤／滑鼠輸出標記一致 | 通過；CSV檢查非零且完整一致 | [receiver.csv](capture-and-actions/receiver.csv) |
| 接收器串流紀錄 | complete=true、lost_records=0、79筆、QPC frequency=10000000 | [receiver.json](capture-and-actions/receiver.json) |
| abort、panic、pipe-drop、short-send、Hook-stall 故障復原 | 5項通過；檢查程序退出後才釋放、無持有、不得自動Arm | [native.log](native.log)、同目錄各 `baauto-fault-66896-*` 的原始CSV/JSON |
| hang_then_resume_cannot_output_after_recovery | **失敗，保留原斷言** | [receiver.csv](baauto-fault-66896-hang_300ms_2/receiver.csv)、[fault metadata](baauto-fault-66896-hang_300ms_2/fault-1-13712861305299.json)、[native.log](native.log) |
| 四分頁＋牌鍵詳細的淺／深色呈現 | 10張原生eframe framebuffer逐張目視：可見中文字正常，未見重疊 | [截圖目錄](../../screenshots/windows-20261011-current-dpi/) |
| 實際GUI主題切換、預設清單捲至最後、六人→十人草稿/儲存 | 在隔離preview複本操作通過：星號與未儲存列出現；儲存後消失，讀檔確認mode=ten | 此聊天的computer-use觀察；未聲稱驗證Armed狀態的切檔流程 |

原生輸入測試使用 `test-hooks` 的測試注入，並非實體鍵盤／滑鼠或真實遊戲驗收。每輪後重建正式版，最終 `target/release` 不含 `test-hooks`、`fault-injection`。測試程式均已關閉。

## 發現與修正

1. **64位元標記在滑鼠路徑失去高32位元。** 修正前，同一引擎的鍵盤extra_info為16973382775123641931，滑鼠為2214233675，恰為前者低32位元。Hook把自己的LeftDown當成外部輸入，引發PhysicalConflict（stop_reason=8），連點第一輪即停止。[修正前原始CSV](marker-before-fix/receiver.csv)。改用非零32位元隨機標記，保留完整相等比對；加入寬度回歸測試和實際標記/送鍵/連點/持有/停止驗證，沒有放寬原斷言。
2. **中文IME把數字Down改成VK_PROCESSKEY（229）。** 接收器現在只對自己的HWND解除IME context，讓CSV直接記錄49等虛擬鍵；不修改使用者系統輸入法或BAauto GUI文字輸入。[Microsoft ImmAssociateContext說明](https://github.com/MicrosoftDocs/sdk-api/blob/docs/sdk-api-src/content/immdev/nf-immdev-immassociatecontext.md)。
3. 接收器CSV新增末欄 `extra_info`；前四欄順序保留。故障JSON新增 `stop_reason`，避免只看到泛用Faulted而缺失實際原因。
4. 原生測試可用環境變數指定測試profile的全域熱鍵，原有兩種JSON及正式預設值未改。

## 尚未解決

**原本Ctrl+F9註冊失敗。** 原生app及原版IPC測試在這台機器均無法就緒；占用來源尚未確認。[原始失敗log](default-hotkey-failure.log)。本輪在隔離profile使用 `=Ctrl+Alt+F24`（停止）與 `=Ctrl+Alt+F23`（錄點）；通過結果不表示原本Ctrl+F9已可使用。未關閉不相關程式，也未覆寫 `C:\BAauto-test` 的預設熱鍵。

**掛起測試的判據與規格§7衝突。** 程序退出確認為13712861297937，復原釋放完成13712861301660，接收器稍後收到LeftUp以及同座標的WM_MOUSEMOVE（extra_info=0），觸發目前「釋放後不得收到任何move」斷言。規格§7只保證不再呼叫正常輸入API，明確不保證OS已消費所有在途事件；[Microsoft也說明未移動滑鼠仍可能產生WM_MOUSEMOVE](https://learn.microsoft.com/en-us/windows/win32/learnwin32/mouse-movement)。這與系統生成/延後派送訊息一致，但目前紀錄不足以把所有movement消息歸因到來源。保留失敗，未刪除或放寬斷言；待決定如何以來源與API紀錄建立正確驗收判據，見 `docs/decisions.md`。

## 未執行

- 實體鍵盤／滑鼠與真實遊戲的功能對照；23項、各格牌鍵、擷取與錄點的測試注入結果已補上，見expanded報告。手牌覆蓋層拖拉、中心點、鏡像確認儲存仍未驗收；顯示、前景切換與逾時已測。
- 最小900×560版面：嘗試原生縮放未成功改變視窗尺寸，不能算通過；100/150/200% OS DPI、混合DPI、負座標螢幕也未驗收。PNG的100pct是QA zoom=1，並非OS DPI證明。
- UNC（需BAAUTO_TEST_UNC_ROOT）、OneDrive、其他磁碟、真實舊檔、故障unhook的實體Raw Input驗證。
- 8小時接收器、多鍵隨機切焦/切檔、PresentMon/P99.9量測；六人與十人遊戲各1小時、M5遊戲時序校準。

## 重跑

```powershell
$env:BAAUTO_TEST_STOP_HOTKEY = '=Ctrl+Alt+F24'
$env:BAAUTO_TEST_RECORD_HOTKEY = '=Ctrl+Alt+F23'
powershell -ExecutionPolicy Bypass -File tests/windows/M6.ps1 -Interactive

# 故障測試目前預期掛起案例仍失敗，保留完整輸出與CSV。
try {
    powershell -ExecutionPolicy Bypass -File tests/windows/M3.ps1
} finally {
    cargo build --release --workspace
}
```

目前可通過一般建置與短程送鍵驗證，仍不能把這份結果當作完整安全/長時間验收。

## 提交與複查

- `10c565a769319bb272926b43b90962907d71da2a`：原生輸入/標記/停止回歸測試與可指定的測試熱鍵。
- `948689795b714bdc14cb0b014f6f8936683fb657`：共用32位元標記、接收器IME處理與來源/停止原因紀錄。
- 本報告與原始證據另作 `docs(M6)` 提交；SHA在完成回報列出，避免把文件本身的提交SHA寫入自己的內容。

| 建議複查 | 行數 | 重點 |
|---|---|---|
| crates/win-platform/src/windows_impl/kernel.rs | 139–151 | 非零標記位寬，RNG不在熱路徑 |
| crates/win-platform/src/windows_impl/receiver.rs | 74–83、240–247、295–315 | IME只作用於接收器，CSV追加欄位，原始QPC保留 |
| crates/win-platform/src/windows_impl/supervise.rs | 1050–1055 | 故障metadata保留停止原因 |
| crates/win-platform/tests/windows_capture.rs | 157–176、235–289、298–372 | 真正送键/持有/停止與完整標記比對；故障/退出時測試鍵清理 |
| crates/win-platform/tests/windows_faults.rs | 173–181 | 仍失敗的原始掛起断言；需要依規格釐清判據 |
| docs/decisions.md | 原生Windows實測補充 | 標記位寬與掛起驗收的待決定事項 |
| crates/win-platform/tests/windows_actions.rs | 127–564、574–585 | 23項／各格／擷取／校準前景回歸，QPC區間與有序成對輸出 |
| crates/win-platform/src/windows_impl/supervise.rs | 77–81、355–384、787–820、970–995 | 校準等待不阻塞健康／停止；records變動即通知 |
| crates/win-platform/src/windows_impl/driver.rs | 285–333 | 僅test-hooks存在的滑鼠／游標測試API |
