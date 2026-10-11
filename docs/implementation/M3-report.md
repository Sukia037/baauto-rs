# M3 實作與驗收狀態

已實作 managed 子程序、overlapped Named Pipe、嚴格訊框與握手、新 session 的獨立停止 Event、監督純 FSM、Raw Input 對照、緊急熱鍵、設定切換及七種 fault-injection 規則。只有取得實際程序 handle 的退出證明才允許監督釋放；釋放回傳數量與 QPC 寫入 fault JSON，帳本／候選集合及環形紀錄另存 CSV。故障重啟停在 Faulted，需手動 Enable；失焦回來使用新 session。

## 驗收對應

IPC 的版本、截斷、未知欄位、過大訊框與 compiled payload 驗證有測試。任務列出的八個 fsm_* 測試全部通過；另涵蓋晚到 Armed、釋放失敗封鎖 Arm、跨輪 Raw Input 配對、失焦及 Disarmed 基線，共 15 個 FSM 測試。獨立複查的五個問題已修正：Raw Input 晚到與配對額度過期、Rehook 確認基線、遊戲 PID 變更時重新監看、切預設按鍵檔重新註冊全域熱鍵、釋放失敗不得再次啟用。

## 指令與限制

macOS 已通過 cargo fmt --check、cargo clippy --workspace --all-targets -- -D warnings、cargo test --workspace，以及 cargo clippy --workspace --all-targets --all-features --target x86_64-pc-windows-gnu -- -D warnings。可攜測試總計 98 個（core 40、config 15、protocol 25、win-platform 18）；Windows 專用部分未在本機執行。

Windows 請執行 tests/windows/M3.ps1。腳本建立測試 binaries 並跑六個 ignored 故障測試；測試有失敗清理保護，檢查 Faulted、不自動 Arm、輸出鍵放開、receiver 最後狀態、fault CSV/JSON、退出早於釋放，以及 hang 復原後無滑鼠 Down/Move。unhook 要實體 Raw Input，腳本列出手動步驟。父程序強制退出、緊急停止延遲、設定切換、100 次 Arm/Stop handle 穩定性仍需 Windows 實跑，不能視為已驗收。

## 草案差異與決策

所有命令包括 Hello 都帶 cmd_seq，所有回覆帶 session/cmd_seq，比草案更嚴格。CompiledConfig 轉成可驗證的 wire payload，非直接傳本機所有權。共享版面升至 2，新增停止原因和同集合的 Hook 計數。監督逾時暫用保守 1000ms，尚需負載定案。失敗釋放封鎖 Arm，故障快照保留在 logs；不重試整批。詳見 docs/decisions.md。

## 建議複查

crates/protocol/src/supervisor.rs（停止順序與健康配對）、crates/win-platform/src/windows_impl/supervise.rs（Child 的 Drop/recover、execute_effects）、managed.rs（父程序消失及設定安装）、pipe.rs（overlapped 取消後等待完成）、driver.rs（退出證明及釋放）、health_window.rs（全域熱鍵重註冊與 Raw Input）。

## 提交

43a4900：先建立 IPC／FSM 驗收；cd8d2c0：監督與 managed 實作；第三筆提交：回歸修正、Windows 故障腳本及本報告（見 git log）。
