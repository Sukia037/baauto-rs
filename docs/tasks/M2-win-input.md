# M2 · win-input：最小 Windows 路徑

> 規格依據：v0.4 §2、§3、§4、§5、§6、§8、§12B
> 前置：M1　｜　環境：雲端寫＋編譯檢查，本機 Windows 測試　｜　建議 effort：高　｜　完成後複查：**必做**

## 目標

把 M1 的引擎接上真的 Windows：Hook 執行緒收實體輸入 → SPSC 佇列 → Engine 執行緒 → Driver 用 `SendInput` 送出。同時建好共享記憶體（進度計數、輸出帳本、環形紀錄）。先用自己寫的「測試接收器」當目標視窗，不碰真的遊戲。

M2 結束時，引擎可以單獨執行（standalone 模式），延遲和順序都能從環形紀錄量出來。監督程序、IPC、故障復原是 M3 的事。

## 範圍

### 要做
- `crates/protocol::shm`：共享記憶體版面（標頭、進度計數、輸出帳本、環形紀錄）與讀寫協定。
- `crates/win-platform`：
  - `hook`：Hook 執行緒、訊息迴圈、觸發判定、實體鍵表、press 配對、焦點追蹤。
  - `timer`：高解析度 waitable timer；不可用時回報降級。
  - `event`：auto-reset 喚醒 Event、每 session 一個的 manual-reset 停止 Event。
  - `driver`：`INPUT` 陣列、`SendInput`、座標換算、自家標記、帳本先記後送。
  - `shm`：建立／開啟具名共享記憶體的封裝。
- `bins/baauto-engine`：standalone 模式。三條執行緒：Hook、Engine、Control（M2 的 Control 只讀 CLI 參數和處理 Ctrl+C，M3 換成 Named Pipe）。
- `tools/bench/receiver`：測試接收器，一個簡單的 Win32 視窗，用 QPC 時間記下收到的鍵盤與滑鼠訊息。
- Loom 模型三個：喚醒協定、帳本順序、環形紀錄的單筆讀寫。

### 不做
- 控制程序、Named Pipe、監督、停止逾時與強制終止（M3）。
- 舊設定匯入（M4）。遊戲一律當成全螢幕 16:9：遊戲畫面＝遊戲視窗所在的整個螢幕（`MonitorFromWindow`），不處理視窗化、黑邊，沒有內容區校準。
- 真的遊戲測試、PresentMon、DXGI（M5）。

## 執行緒與資料流

```text
實體輸入 ─> [Hook 執行緒] ─rtrb A─> [Engine 執行緒 + Driver] ─SendInput─> 目標視窗
                 │  └─ 溢位／失焦：直接 SetEvent(停止)        │
                 └─ hook 進度 ──────> 共享記憶體 <──── 帳本、紀錄、進度
[Control 執行緒] ─rtrb B─> Engine；安裝設定時用 PostThreadMessage 通知 Hook
```

## 共享記憶體版面（`protocol::shm`）

```rust
#[repr(C)] pub struct ShmHeader {
    pub magic: u64, pub layout_version: u32, pub size: u32,
    pub session: AtomicU32, pub config_version: AtomicU32,
    pub engine_state: AtomicU32,          // EngineState 的編碼，帶 session
    pub qpc_frequency: u64,
}
#[repr(C)] pub struct ShmProgress {
    pub scheduler: AtomicU64, pub action: AtomicU64,
    pub hook_loop: AtomicU64, pub hook_last_call_qpc: AtomicU64,
    pub hook_physical_events: AtomicU64,  // Hook 看到的實體事件數，M3 拿來和 Raw Input 對照
    pub overflow_count: AtomicU64, pub degraded_flags: AtomicU32,
}
#[repr(C)] pub struct ShmLedger {
    pub held_keys: [AtomicU64; 4],        // 256 個 VK
    pub held_mouse: AtomicU32,            // bit0 = 左鍵
    pub seq: AtomicU64,                   // 每次改動 +1
}
#[repr(C)] pub struct ShmRing { pub write_seq: AtomicU64, pub entries: [LogRecord; RING_LEN] }
#[repr(C)] pub struct LogRecord {        // 每個欄位都是原子型別，避免跨程序資料競爭
    pub seq: AtomicU64,                   // 最後寫、Release；讀取端前後各讀一次確認沒被覆寫
    pub kind: AtomicU32, pub session: AtomicU32, pub action: AtomicU64, pub stage: AtomicU32,
    pub hook_enter: AtomicU64, pub enqueue: AtomicU64, pub engine_receive: AtomicU64,
    pub deadline: AtomicU64, pub send_begin: AtomicU64, pub send_end: AtomicU64,
    pub inserted: AtomicU32, pub expected: AtomicU32, pub queue_depth: AtomicU32,
}
```

- 只有引擎程序寫（標頭由建立者在 Arm 前寫好）。讀取端照 `seq` 推算遺失筆數。
- `RING_LEN` 至少 8192。
- 靜態斷言結構大小與對齊；版面版本不符時拒絕開啟。
- standalone 模式由引擎自己建立區段；M3 改成控制程序建立、引擎開啟。名稱用 `Local\BAauto-rs-<隨機字串>`。

## Hook 執行緒

callback 本身只呼叫一個純函式 `decide(&HookState, RawEvent) -> Decision`，`decide` 不依賴 Windows，在 Linux 上做單元測試。callback 依序做：

1. 帶自家 `dwExtraInfo` 標記 → 放行，不更新任何狀態。
2. 其他程式注入（`LLKHF_INJECTED`／`LLMHF_INJECTED`）→ 放行，不觸發。（測試用 feature `test-hooks` 可以開例外，見下方）
3. 滑鼠移動 → 立刻放行（只在錄點模式才記錄，錄點是 M6）。
4. 更新實體鍵表與修飾鍵狀態，`hook_physical_events` 加 1。實體鍵已經是按下時又收到 Down，就是 OS 自動重複。
5. Up：如果這個鍵的 Down 當初被攔截，就用當初的 `press` 產生 `TriggerUp` 並攔截，**不管現在焦點或設定是什麼**。
6. Down：已 Arm、session 有效、遊戲在前景、（滑鼠鍵還要游標在遊戲矩形內）、觸發表命中 → 產生新的 `press`、記進配對表、push `TriggerDown`、攔截。游標位置由 `MSLLHOOKSTRUCT.pt`（滑鼠）或 `GetCursorPos`（鍵盤觸發）取得，換算成 `NormPoint`。
7. 修飾鍵放開使某個 press 的 chord 失效 → push `PressCancelled`。
8. 使用者實體按下的鍵正好在帳本的持有集合裡 → push `PhysicalConflict`。
9. push 失敗 → 設 `overflow_count`、直接 `SetEvent(停止)`。之後這個 session 的觸發一律不再啟動，但已攔截的 Down 對應的 Up 照樣攔截。
10. 寫 `hook_last_call_qpc`。

其他規定：
- 觸發表是 Hook 執行緒自己的變數（固定大小陣列）。Control 用 `PostThreadMessage` 把新表交給 Hook 的訊息迴圈，在兩次 callback 之間換掉；舊表送回 Control 執行緒釋放。只在 Disarmed 時換。
- 焦點：`SetWinEventHook(EVENT_SYSTEM_FOREGROUND, …, WINEVENT_OUTOFCONTEXT)`，加上只針對目標 PID 的 `EVENT_OBJECT_LOCATIONCHANGE`。前景改變時用 `GetWindowThreadProcessId` 比對快取的 PID；PID 不明時才用 `QueryFullProcessImageNameW` 查一次名稱。結果寫進 Hook 自己的狀態，並 push `Focus` 事件（帶 `GeometryVersion`）。
- 失去焦點時，除了 push `Focus`，也直接 `SetEvent(停止)`，不讓停止排在一般事件後面。
- 遊戲畫面：取遊戲視窗所在螢幕的矩形。螢幕換了或解析度改了就遞增 `GeometryVersion`。遊戲視窗矩形不等於螢幕矩形、或螢幕不是 16:9 時，回報 `NotFullscreen16x9` 給控制程序顯示警告，不做補償。
- 探測：收到 `WM_APP_PROBE` 就把 `hook_loop` 加 1。
- 執行緒優先權 `THREAD_PRIORITY_TIME_CRITICAL`。callback 包 `catch_unwind`。

## Engine 執行緒

照規格 §5、§6：

```text
loop {
  WaitForMultipleObjects([stop(本 session), wake(auto-reset), timer, control])
  若 stop 已觸發 → engine.request_stop(...)
  處理最多 max_events_per_round 個輸入事件和控制命令
  最多 max_batches_per_round 次：next_batch → Driver 送出 → commit
  更新進度計數；設定 timer 到 engine.next_wake()
  佇列還有東西 → 直接下一輪；否則睡前再檢查一次佇列
}
```

- 不手動 `ResetEvent` 喚醒 Event。
- 進入 Stopped 後不再等已觸發的舊停止 Event（避免空轉），改等控制和健康。
- 換設定時，Engine 把舊的 `Arc<CompiledConfig>` 透過佇列交回 Control 執行緒釋放，大型解構不落在 Engine 執行緒。
- 收到 `TriggerDown` 時，把 Hook 填的時間戳原樣帶進紀錄。

## Driver

- `OutEvent` → 預先配置的 `INPUT` 陣列。鍵盤同時填 `wVk` 和 `wScan`（`MapVirtualKeyW`）。要不要加 `KEYEVENTF_SCANCODE`，用接收器和之後的遊戲實測決定，先寫進 `docs/decisions.md`。
- 滑鼠用 `MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`。換算函式是純函式，要保證送出的正規化座標落在目標像素上（含負原點、多螢幕）。
- 每筆 `dwExtraInfo` 帶自家標記（啟動時隨機產生的 64 位元值）。
- 送出前：看一次停止 Event（`WaitForSingleObject(stop, 0)`）和前景旗標。
- 帳本：送出 Down 之前先設位元、遞增 `seq`；`SendInput` 回傳筆數等於送出筆數，才清除對應的 Up 位元。回傳筆數不足 → `SubmitOutcome::Short`，位元不清。
- 寫環形紀錄：`deadline`、`send_begin`、`send_end`、`inserted`、`expected`。

## 測試接收器 `tools/bench/receiver`

- 一個可調大小的 Win32 視窗，執行檔名稱可以改，讓引擎把它當成「遊戲」。
- 用 QPC 記下 `WM_KEYDOWN`／`WM_KEYUP`／`WM_LBUTTONDOWN`／`WM_LBUTTONUP`／`WM_MOUSEMOVE`，輸出 CSV。
- 視窗上即時顯示最近 20 筆事件，方便手動看。
- 依啟動參數在視窗上畫出手牌格（`--hand six|ten --side right|left`），位置用範本設定檔的預設手牌矩形，讓人可以把游標放到某一格上測牌鍵。

## standalone 模式

```text
baauto-engine --standalone --config tests/fixtures/m2-basic.json --target-exe baauto-receiver.exe [--max-run-secs 600]
```

- `--config` 吃 M1 格式的設定檔，引擎自己呼叫 `config::compile`。M3 之後改由控制程序編譯再傳過來。
- 建立 `tests/fixtures/m2-basic.json`：以六人範本為底，手牌在右邊，加一個普通點位、一個牌鍵熱鍵；timing profile 用明顯偏保守的手動值，`verified: false`。
- 建立 `tests/fixtures/m2-ten-left.json`：十人模式、手牌在左邊，其餘同上。
- 自己建立共享記憶體和停止 Event，啟動後自動 Arm。
- Ctrl+C 或時間到 → 觸發停止，等 Stopped，結束時印出共享記憶體名稱和紀錄筆數。
- 另有 `--dump-ring <檔案>`：把環形紀錄倒成 CSV。

## 測試與驗收

**Linux（雲端）**

- [ ] `decide_*` 系列：每一條 Hook 規則一個測試，至少包括：自家注入放行、外部注入不觸發、焦點中途改變時 Up 仍配對攔截、修飾鍵放開產生取消、自動重複、實體衝突、溢位後只攔不觸發。
- [ ] 座標換算：多螢幕、負原點、不同 DPI 比例下，正規化座標換回像素的誤差為 0。
- [ ] `shm` 版面：大小與對齊的靜態斷言；用兩條執行緒模擬寫入端快、讀取端慢，讀取端算出的遺失筆數正確，且不會讀到被覆寫一半的紀錄。
- [ ] Loom：`wake_protocol_never_sleeps_with_pending_events`、`ledger_is_superset_at_any_kill_point`、`ring_reader_detects_torn_record`。
- [ ] `cargo check --target x86_64-pc-windows-gnu` 通過。

**Windows（本機；需要互動桌面的加 `#[ignore]`）**

- [ ] 接收器在前景時，實體按 W → 接收器收到 1 的 Down 與 Up；接收器不在前景時不攔截。
- [ ] 普通點位按住中切到別的視窗 → 接收器收到 LeftUp，帳本變空。
- [ ] 按住普通點位熱鍵時切換焦點再切回來，放開熱鍵 → Up 仍被攔截，不會有鍵留在按下狀態。
- [ ] 牌鍵：游標放在接收器畫的第 2 格上按牌鍵熱鍵 → 接收器收到 2 的 Down 與 Up；游標在手牌外 → 什麼都沒收到。`m2-basic.json`（六人、右）和 `m2-ten-left.json`（十人、左）各測一次，每一格都試。
- [ ] `test-hooks` feature 下，用測試程式灌入大量觸發，佇列滿 → 停止 Event 觸發、session 作廢、沒有任何鍵卡住。
- [ ] 用 `TerminateProcess` 在普通點位按住中砍掉引擎 → 從共享記憶體讀到的帳本包含左鍵（M3 才會真的去放開）。
- [ ] `GetThreadPriority` 確認只有 Hook 執行緒是 TIME_CRITICAL。
- [ ] CPU 滿載（例如同時跑一個吃滿所有核心的程式）下連按 5 分鐘，Hook 沒有被移除（每次實體按鍵都有對應紀錄）。
- [ ] 量測：連按 1000 次，從環形紀錄算出 `hook_enter → send_end` 的 P50、P99、最大值，寫進回報。這一步不設門檻，只要有數字。

## 手動驗收（給使用者）

1. `cargo build --release`。
2. 開 `baauto-receiver.exe --hand six --side right`，再開 `baauto-engine --standalone --config tests/fixtures/m2-basic.json --target-exe baauto-receiver.exe`。
3. 點一下接收器讓它在前景，依序試：W（技能）、2（連點）、Down（卡幀）、普通點位熱鍵；再把游標移到畫出來的第 1、2、3 格上各按一次牌鍵熱鍵。接收器畫面上要看到對應的事件，而且放開後沒有殘留。
4. 普通點位按住中按 Alt+Tab，確認接收器收到 LeftUp。
5. 換成 `--hand ten --side left` 和 `m2-ten-left.json` 再試一次牌鍵，五格都要對。
6. Ctrl+C 結束引擎，`--dump-ring` 輸出的 CSV 每一列都有 `deadline` 和 `send_end`。

## 複查清單（必做，給 Astra／Opus）

- Hook callback：從進入到回傳，有沒有任何配置、鎖、log、可能阻塞的呼叫？`catch_unwind` 是否包住全部？
- Up 配對：焦點、設定、session 在 Down 和 Up 之間改變時，Up 是否一定照當初的決定處理？
- 喚醒協定：有沒有手動 `ResetEvent`？每輪到上限時會不會睡著而留下待處理事件？
- 停止 Event：是否每個 session 新建、同一 session 內不清除？Stopped 後會不會空轉？
- 帳本：Down 的位元是否在 `SendInput` 之前設好？Up 的位元是否只在回傳筆數正確時清除？
- 環形紀錄：所有跨程序欄位是否都是原子型別？`seq` 是否最後寫、用 Release？
- `INPUT` 結構：大小、旗標、`dwExtraInfo`、座標換算是否正確？
- 舊觸發表是否在 Control 執行緒釋放？

## 貼給 Codex 的開場訊息

```text
請完成里程碑 M2。先讀 AGENTS.md、docs/spec/BAauto-rs-融合架構-v0.4.md（重點 §2–§6、§8、§12B）和 docs/tasks/M2-win-input.md。M1 已完成，core 的 Engine 介面不要改語意；需要改介面時先說明。

這一步要做：protocol::shm、win-platform（hook、timer、event、driver、shm）、baauto-engine 的 standalone 模式、tools/bench/receiver，以及三個 Loom 模型。

做法：
1. 先把 Hook 的判斷寫成不依賴 Windows 的純函式 decide()，連同 shm 版面和座標換算，在 Linux 上寫好測試並通過。
2. 再寫 Windows 封裝與執行緒。雲端環境跑不了 Windows 測試時，至少讓 cargo check --target x86_64-pc-windows-gnu 通過，並把 Windows 測試標好 #[cfg(windows)]／#[ignore]，在回報裡寫清楚手動怎麼跑。
3. 每個 unsafe 都要有 // SAFETY: 說明。

完成後照 AGENTS.md 的格式回報，並列出建議複查的檔案與行數範圍。
```
