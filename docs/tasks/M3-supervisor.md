# M3 · supervisor：控制程序、停止協定、故障復原

> 規格依據：v0.4 §2、§6、§7、§8、§10、§12B
> 前置：M2　｜　環境：雲端寫＋Linux 測純邏輯，本機 Windows 測故障注入　｜　建議 effort：高　｜　完成後複查：**必做**

## 目標

加上控制程序 `baauto-app` 和它的監督執行緒，讓引擎變成被管理的子程序。完成規格裡的停止與復原協定：先要求合作停止，逾時就終止整個引擎程序，**確認程序已經結束**之後才依帳本放開按鍵。任何故障都不能留下卡住的鍵，也不能自動恢復連點。

M3 的 `baauto-app` 先做成命令列版（headless），完整 GUI 是 M6。

## 範圍

### 要做
- `crates/protocol::ipc`：控制訊息、編碼、版本握手。
- 監督邏輯寫成純狀態機 `SupervisorFsm`，在 Linux 上測。
- `bins/baauto-app`：建立共享記憶體與停止 Event、啟動引擎、Named Pipe 伺服器、監督執行緒、緊急熱鍵、Raw Input 對照、headless 介面。
- `bins/baauto-engine`：managed 模式。Control 執行緒改成 Named Pipe 用戶端；監看控制程序存活。
- 故障注入（feature `fault-injection`，只在測試建置開啟）。
- 監督程序自己的釋放 Driver（沿用 `win-platform::driver`，但帶監督自己的標記）。

### 不做
- GUI、錄點（M6）。
- 量測報表（M5）。

## IPC（`protocol::ipc`）

- Named Pipe、overlapped I/O。訊框：4 bytes 長度＋`serde_json`。不在逐鍵路徑上，所以選好除錯的格式。
- 每則命令帶遞增的 `cmd_seq`；每則回覆帶 `session` 和對應的 `cmd_seq`。

```rust
pub enum AppToEngine {
    Hello { protocol: u32, shm_layout: u32 },
    ApplyConfig { cmd_seq: u64, version: ConfigVersion, compiled: CompiledConfig },
    Arm { cmd_seq: u64, session: SessionId, stop_event: HandleValue },
    Stop { cmd_seq: u64, session: SessionId, reason: StopReason },
    Probe { cmd_seq: u64 },        // 請 Control 對 Hook 送 WM_APP_PROBE
    Rehook { cmd_seq: u64 },       // 只允許在 Disarmed
    Shutdown { cmd_seq: u64 },
}
pub enum EngineToApp {
    HelloAck { protocol: u32, shm_layout: u32 },
    ConfigInstalled { cmd_seq: u64, version: ConfigVersion },   // Hook 與 Engine 都裝好才回
    KeysReleased { session: SessionId },                         // 相關實體鍵都放開了
    Armed { cmd_seq: u64, session: SessionId },
    Stopped { session: SessionId, reason: StopReason },
    Faulted { session: SessionId, kind: FaultKind },
    Status { session: SessionId, state: EngineStateCode, counters: CountersSnapshot },
    Rejected { cmd_seq: u64, why: RejectReason },
}
```

- 晚到的 `Stopped`、舊的 `Armed` 一律用 session 和 `cmd_seq` 過濾，不能抵銷新的停止。
- 停止 Event 不經過管線：控制程序為每個 session 建一個 manual-reset Event，用 `DuplicateHandle` 複製到引擎程序，再把 handle 值放進 `Arm`。

## 啟動與啟用流程

1. 控制程序建立共享記憶體（`Local\BAauto-rs-<隨機>`）並寫好標頭。
2. 用 `CreateProcessW` 啟動 `baauto-engine --managed --shm <名稱> --pipe <名稱> --parent <pid>`。
3. `Hello` 握手：協定版本或版面版本不符就結束引擎並回報。
4. `ApplyConfig` → 等 `ConfigInstalled`（Hook 與 Engine 都安裝完同一版本）。
5. 等 `KeysReleased`。
6. 緊急熱鍵註冊成功才算就緒；註冊失敗就不能顯示「已就緒」，也不允許 Arm。
7. `Arm`（新 session、新停止 Event）→ 等 `Armed`。

## 失焦之後怎麼重新啟用

使用者按「啟用」之後，控制程序記住「使用者要它運作」。失焦會結束當前 session（引擎釋放所有按鍵）；焦點回到遊戲、幾何資訊確認過、相關按鍵都已放開時，監督自動建立**新的** session 並 Arm，不恢復任何舊動作。這樣切出遊戲再切回來不用每次重按啟用，和 AHK 版的使用習慣一致。

Faulted 不適用這條：故障之後一定要使用者手動重新啟用。這個做法寫進 `docs/decisions.md`，標成「預設，可改」。

## 監督執行緒

- 專用 OS 執行緒，有自己的訊息迴圈：`RegisterHotKey` Ctrl+F9（緊急停止）、Ctrl+F10（錄點，M3 先只記錄、不動作）。
- 健康取樣週期與逾時都可以設定，先放保守的預設值，標成「待負載測試定案」。三種訊號分開判斷：
  - `scheduler` 不動 → 引擎迴圈卡住 → 停止協定。
  - `hook_loop` 對探測沒反應 → Hook 執行緒卡住 → 停止協定。
  - Hook 可能被拔：控制程序用隱藏視窗註冊 Raw Input（`RIDEV_INPUTSINK`），只計算實體裝置的事件。遊戲在前景、Raw Input 看到綁定鍵的實體事件、但 `hook_physical_events` 沒有增加 → 懷疑 Hook 被移除 → 停止協定 → 回到 Disarmed 後才 `Rehook`，並要求使用者按一個鍵確認。沒有實體輸入時不判定失效。
- 監督執行緒不等 GUI 繪圖、不等檔案寫入。

## 停止協定（規格 §7）

```text
觸發：Ctrl+F9、健康逾時、使用者停止、引擎回報故障、管線斷線
1. 禁止重新 Arm；SetEvent(本 session 停止 Event)；同時送 Stop 訊息（備援）
2. 等 Stopped(session)：看共享記憶體 engine_state 或管線訊息，最多 recovery_timeout
3. 等到了 → 結束（Disarmed）
4. 逾時 → TerminateProcess(引擎) → WaitForSingleObject(引擎程序) 直到真的結束
5. 讀帳本：標頭的 magic、版面版本、session 都正確 → 放開帳本裡的鍵；
   不正確 → 放開這個 session 的候選鍵集合（Arm 前由設定算出）
   用監督自己的 Driver 送，檢查回傳筆數，記錄結果
6. 把環形紀錄倒成 logs/fault-<時間>.csv
7. 狀態保持 Faulted；重啟引擎但停在 Disarmed；不重播、不自動恢復
```

## 引擎端（managed 模式）

- Control 執行緒同時等管線和控制程序的 process handle（`OpenProcess` 父程序 PID）。任何一個斷掉 → `request_stop(SupervisorLost)` → 等 Stopped → 結束程序。
- 引擎迴圈每一圈包 `catch_unwind`：panic → `request_stop(Fault)`，盡量走正常釋放 → 回報 `Faulted` → 以非 0 結束。`Drop` 裡的釋放只是輔助。

## `SupervisorFsm`（純邏輯）

輸入：健康取樣、管線訊息、計時到期、程序已結束、熱鍵、Raw Input 計數。
輸出：`SetStop`、`SendStop`、`Terminate`、`WaitExit`、`ReleaseFromLedger`、`ReleaseCandidates`、`DumpRing`、`RestartDisarmed`、`ReportReady` 等動作。

Windows 層只負責執行這些動作和回饋結果。所有故障順序都先在 Linux 上用 FSM 測過。

## 故障注入（feature `fault-injection`）

引擎參數 `--inject <規則>`，至少支援：

| 規則 | 效果 |
|---|---|
| `panic@N` | 第 N 批送出前 panic |
| `abort@N` | 第 N 批送出前 `std::process::abort()` |
| `hang:300ms@N` | 第 N 批之前讓引擎迴圈停 300ms，然後**繼續執行** |
| `short@N` | 第 N 批假裝 `SendInput` 只插入一部分 |
| `pipe-drop@N` | 第 N 批後關掉管線 |
| `unhook@N` | 第 N 批後呼叫 `UnhookWindowsHookEx` |
| `hook-stall` | Hook 訊息迴圈不再處理探測 |

## 測試與驗收

**Linux（雲端）**

- [ ] IPC 編碼與握手：版本不符、訊框截斷、未知訊息。
- [ ] `fsm_cooperative_stop`：合作停止在時限內完成 → 不終止引擎。
- [ ] `fsm_timeout_terminates_then_waits_exit_then_releases`：順序一定是 Terminate → WaitExit → Release，不能顛倒。
- [ ] `fsm_header_mismatch_uses_candidate_set`。
- [ ] `fsm_stale_stopped_from_old_session_ignored`。
- [ ] `fsm_no_auto_resume_after_fault`：故障後沒有任何路徑在使用者 Arm 之前回到 Armed。
- [ ] `fsm_rehook_only_when_disarmed`。
- [ ] `fsm_hotkey_registration_failure_blocks_ready`。
- [ ] `fsm_idle_engine_not_treated_as_stuck`：閒置時只要 `scheduler` 照健康週期前進，就不觸發停止。

**Windows（本機，用 M2 的接收器當目標）**

每一個故障注入情境都要驗證：接收器紀錄裡每個 Down 都有 Up、`GetAsyncKeyState` 沒有殘留、`logs/fault-*.csv` 存在、狀態是 Faulted、沒有自動恢復。

- [ ] `panic@N`、`abort@N`、`short@N`、`pipe-drop@N`、`unhook@N`、`hook-stall` 各一次。
- [ ] **`hang:300ms@N` 在普通點位按住中、同時連點在跑**：監督逾時 → 終止 → 等結束 → 放開。檢查接收器紀錄：監督送出 LeftUp 之後，不能再出現任何來自引擎的事件。
- [ ] 直接在工作管理員結束控制程序 → 引擎自己停止、放鍵、結束。
- [ ] Ctrl+F9 在正常運作時：合作停止，回報 `stop_request → stop_ack` 的時間。
- [ ] 換設定：運作中送 `ApplyConfig` → 先停止、確認釋放、安裝、等按鍵放開，才能再 Arm。
- [ ] 切換設定檔：`m2-basic.json`（六人、右）→ `m2-ten-left.json`（十人、左）。切換後牌鍵用的是新模式與新手牌位置，切換過程沒有任何殘留按鍵。headless 版用 `--profile <檔案>` 搭配一個切換指令（例如在主控台輸入 `profile <檔案>`）即可，介面在 M6。
- [ ] 連續 Arm／停止 100 次，沒有 handle 洩漏（Task Manager 的 handle 數穩定）。

## 手動驗收（給使用者）

1. 開接收器，再開 `baauto-app --headless --profile tests/fixtures/m2-basic.json --target-exe baauto-receiver.exe`。
2. 正常操作幾種動作後按 Ctrl+F9，確認停止且沒有殘留。
3. 用 `fault-injection` 建置跑 `hang:300ms` 情境：按住普通點位熱鍵，等引擎卡住，確認接收器最後一個滑鼠事件是 LeftUp。
4. 在主控台切換設定檔（六人 → 十人），確認牌鍵的格數和手牌位置跟著改變。
5. 在工作管理員直接結束 `baauto-engine.exe`，確認控制程序放開按鍵並寫出 `logs/fault-*.csv`。

## 複查清單（必做，給 Astra／Opus）

- 終止與釋放的順序：有沒有任何路徑在 `WaitForSingleObject` 確認結束之前就送釋放？
- session 與 `cmd_seq`：舊回覆能不能讓狀態機誤以為新 session 已停止或已 Arm？
- 停止 Event：每個 session 新建、複製到引擎的方式正確；舊的 handle 什麼時候關，會不會被還在等待的一方用到？
- 監督執行緒：有沒有可能被 GUI、檔案或管線寫入卡住？
- Raw Input 對照：在沒有實體輸入或遊戲不在前景時，會不會誤判 Hook 失效？
- 引擎端：控制程序消失時，是否一定會停止並結束，而不是無人監督繼續輸出？
- 帳本與候選集合：標頭驗證是否涵蓋 magic、版面版本、session？

## 貼給 Codex 的開場訊息

```text
請完成里程碑 M3。先讀 AGENTS.md、docs/spec/BAauto-rs-融合架構-v0.4.md（重點 §6、§7、§8、§10）和 docs/tasks/M3-supervisor.md。M1、M2 已完成。

這一步要做：protocol::ipc、純邏輯的 SupervisorFsm、baauto-app 的 headless 模式（共享記憶體、停止 Event、啟動引擎、Named Pipe、監督執行緒、緊急熱鍵、Raw Input 對照）、baauto-engine 的 managed 模式，以及 fault-injection feature。

做法：
1. 先把 SupervisorFsm 和 IPC 寫成純邏輯，任務檔列的 fsm_* 測試在 Linux 上全部通過。終止與釋放的順序是這一步最重要的保證。
2. 再寫 Windows 端。雲端跑不了 Windows 測試時，讓 cargo check --target x86_64-pc-windows-gnu 通過，並把故障注入情境整理成一個可以在 Windows 上一鍵執行的測試腳本，寫清楚怎麼跑、怎麼判斷通過。

完成後照 AGENTS.md 的格式回報，並列出建議複查的檔案與行數範圍。
```
