# AGENTS.md — BAauto-rs

給在這個 repo 工作的 coding agent（Codex／GPT-6.1 Sol 等）看的規則。每個任務開始前先讀完這份，再讀當次的任務檔。

## 專案是什麼

BAauto-rs 是 Blue Archive PC 版的輸入輔助工具，用 Rust 重寫原本的 AutoHotkey v2 腳本 `BAauto.ahk`。只在 `BlueArchive.exe` 是前景視窗時，把熱鍵轉成送鍵、連點、卡幀、牌鍵（游標在第幾張牌就送那張的數字鍵）、普通點位等操作。支援六人與十人（制約解除決戰）兩種模式，模式跟著設定檔走。目標是同時做到低延遲、動作順序可預測、任何情況都能可靠停止且不卡鍵。

## 文件的優先順序

1. `docs/spec/BAauto-rs-融合架構-v0.4.md`：架構規格，唯一的事實來源。
2. `docs/tasks/Mx-*.md`：當次里程碑的範圍、介面草案與驗收條件。
3. 這份 `AGENTS.md`：任何任務都不能違反的規則。

任務檔和規格衝突時，以規格為準，並在回報裡指出衝突。規格沒寫清楚的地方，不要自己發明語意：在 `docs/decisions.md` 新增一條「待決定」，寫出選項與你暫時採用的做法，然後繼續。

## Repo 結構

```text
baauto-rs/
  crates/core/          純動作與期限邏輯（不依賴 Windows）
  crates/config/        設定型別、驗證、遷移與編譯（不依賴 Windows）
  crates/win-platform/  Hook、Event、Timer、Driver、共享記憶體的 unsafe 封裝
  crates/protocol/      IPC 訊息、session 與版本握手、共享記憶體版面
  bins/baauto-engine/   引擎程序：Hook／Engine／控制觀測執行緒
  bins/baauto-app/      控制程序：GUI／監督／背景工作
  tools/bench/          測試接收器、紀錄分析、PresentMon 匯入、DXGI 擷取、校準報告
  tests/                重播、同步模型、故障注入、相容性
  points/               範例點位 JSON（含 mode），M1 建立
  keys/                 範本預設按鍵 JSON（預設按鍵.json），M1 建立
  docs/spec/  docs/tasks/  docs/decisions.md
  docs/pages/           圖解頁面（延遲時序、新介面 Demo），只供理解，數字是估計或示範值
  docs/archive/         舊版規格與頁面，只是歷史紀錄，不能當實作依據
  reference/            原本的 AHK 腳本
```

`docs/archive/` 裡的內容和現行規格有衝突（例如還有拖曳），一律以 `docs/spec/` 的最新版為準。

## 絕對不能違反的規則

### 熱路徑
- Hook callback 裡：不加鎖、不配置或釋放記憶體、不寫 log、不讀檔、不呼叫會等待的 API、不重試佇列。只查固定大小的表、更新實體鍵狀態、push 事件、`SetEvent`、回傳。
- Hook callback 一定包在 `std::panic::catch_unwind` 裡，panic 時放行該事件。callback 是 `extern "system"`，panic 穿出去會讓整個程序 abort。
- 引擎迴圈的穩態（處理事件、取批次、送出、提交）不配置記憶體、不呼叫阻塞式 `Sleep`、不同步寫檔。等待只能用 `WaitForMultipleObjects` 加高解析度 waitable timer。
- 時間一律用 QPC 整數（`Ticks`）。紀錄存原始 QPC 值，不換成 `Instant` 或毫秒浮點數再存。

### 所有權與輸出
- 動作狀態只有 Engine 執行緒能改。其他執行緒只能傳命令或讀共享記憶體。
- 正常運作時只有 Engine 執行緒的 Driver 可以呼叫 `SendInput`。監督程序只在「確認引擎程序已經結束」之後，才可以送釋放。
- 每筆合成輸入都要帶自家的 `dwExtraInfo` 標記；Hook 看到這個標記就放行、也不更新實體鍵表。其他程式注入的事件預設不觸發巨集。
- 輸出帳本先記後送：送 Down 之前先在共享記憶體設位元，Up 成功提交之後才清。短送或零送時位元保持設定，進 Faulted，不重試整批。
- 一個 `SendInput` 批次只放語意上允許同時提交的事件。中間需要等待的步驟一定分開送。

### 停止與故障
- 停止協定照規格 §6、§7：每個 session 用新的 manual-reset 停止 Event，同一 session 內不清除、不重用。所有回覆都帶 session，不能把上一輪的 Stopped 當成這一輪的。
- 一般喚醒用 auto-reset Event。不手動 `ResetEvent`；每輪處理量到上限而佇列還有東西時，直接進下一輪；睡前再檢查一次佇列。
- 佇列 push 失敗或來源序號出現缺口：立刻觸發停止，這個 session 作廢。永遠不默默丟掉放開事件。
- 故障後不重播佇列、不自動恢復連點。重新啟用需要明確的 Arm，而且相關按鍵都已經放開。

### 時間與設定
- 程式碼裡不寫死任何遊戲時序數字（16、20、50ms 之類）。所有間隔、按住時間都從 `TimingProfile` 讀。測試可以自己建 profile。
- 不在運作中換設定。換設定一定走：停止 → 確認釋放 → Hook 與 Engine 安裝同一版本 → 回報版本 → 按鍵放開 → 重新 Arm。
- 不用 `ArcSwap` 當跨元件的設定更新機制（GUI 自己顯示用可以）。舊設定的最後一次釋放要交給背景執行緒，不能落在 Hook callback 或 Engine 熱路徑。
- 設定有兩種 JSON：點位檔（`points/*.json`：六人／十人＋點位按鍵清單，功能是普通點位或牌鍵）和預設按鍵檔（`keys/*.json`：23 個預設按鍵＋時間）。手牌位置、範圍、每格按鍵是 `settings.json` 的這台電腦設定。兩種 JSON 都是使用者會直接手改的檔案：時間用毫秒、熱鍵用可讀字串（例如 `Ctrl+F9`）、輸出時欄位順序固定。讀到未知欄位、型別錯誤時，回報 JSON 路徑和原因，不 panic、不默默忽略。
- 六人／十人模式來自點位檔；手牌左右位置、範圍、每一格的按鍵來自 `settings.json`；預設按鍵與時間來自預設按鍵檔。程式碼裡不寫死格數或座標（範本裡的預設值除外）。不要把這些搬到別的檔，也不要把兩種 JSON 合成一個。
- 拖曳功能已經移除（規格 v0.4）。不要再實作任何拖曳相關的動作或參數。
- JSON 可以在任何路徑（規格 §10「任何路徑的 JSON」）：路徑用 `PathBuf`／`OsString`，Windows API 用 `…W` 版本，開檔看內容判斷種類、不看資料夾或檔名，不要假設檔案一定在 `points/` 或 `keys/`。
- 遊戲一律當成全螢幕 16:9（規格 §10）。座標都是遊戲所在螢幕的 0–1；不要實作視窗化、黑邊、內容區校準或座標基準切換。

### 優先權
- Hook 執行緒設 `THREAD_PRIORITY_TIME_CRITICAL`。這是唯一預設提高的優先權。
- 不用 `REALTIME_PRIORITY_CLASS`、不綁 CPU、不開尾段自旋，除非任務檔明確要求做對照實驗。

### unsafe
- `unsafe` 只能出現在三個地方：`crates/win-platform`、`crates/protocol` 的共享記憶體版面、`tools/bench` 的 DXGI 擷取模組。兩個執行檔透過 `win-platform` 的安全封裝呼叫 Win32。`core`、`config` 加 `#![forbid(unsafe_code)]`。
- 每個 `unsafe` 區塊上方寫 `// SAFETY:`，說明前提條件為什麼成立。
- 共享記憶體的結構用 `#[repr(C)]`，只放固定大小的資料和原子型別，不放指標。開頭要有 magic、版面版本、大小和 session。

## 測試規則

- 先寫測試，再寫實作。任務檔「測試與驗收」列出的測試全部要有，名稱盡量照用。
- `core`、`config`、`protocol` 的測試必須能在 Linux 上跑（`cargo test`）。
- 需要真的 Windows 才能跑的測試：放在 `#[cfg(windows)]` 裡；需要人操作或真的遊戲的測試再加 `#[ignore]`，並在檔頭註解寫清楚怎麼手動跑。
- 合併前必須通過：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`。在 Linux 環境時，Windows 專屬的 crate 至少要用 `cargo check --target x86_64-pc-windows-gnu` 編過。
- 不能為了讓測試通過而放寬斷言或刪掉測試。測試和規格衝突時，回報出來。

## Git 提交規則

- 每個里程碑（M1–M6）至少保留 3 個有實質內容的提交；工作較多時可以增加。
- 建議依序拆成：測試與公開介面、功能實作、驗收修正與交付文件。先寫測試的規則仍然適用；不要為了湊數建立空提交或無意義拆分。
- 提交訊息帶里程碑，例如 `test(M1): ...`、`feat(M1): ...`、`fix(M1): ...` 或 `docs(M1): ...`，讓進度與範圍可追蹤。
- 合併時保留這些提交，不把整個里程碑 squash 成一個提交。
- 完成回報列出該里程碑的提交 SHA 與用途；初始設計文件的提交不計入任何里程碑。

## 依賴

- Rust edition 2024，用目前的穩定版。依賴選最新的穩定版本，鎖在 `Cargo.lock`。
- 預期會用到：`windows`（微軟官方綁定）、`rtrb`、`arrayvec`、`serde`／`serde_json`、`thiserror`、`proptest`、`loom`（只在測試）、`eframe`／`egui`（只在 app）。
- 新增上面以外的依賴前，在回報裡說明理由。不要用 `tokio`。
- release profile 保持 `panic = "unwind"`，`catch_unwind` 才有作用。

## 完成時要回報

1. 做了什麼，對應任務檔的哪幾條驗收。
2. 跑了哪些指令、結果如何；哪些測試因為環境限制沒跑（例如需要 Windows），要怎麼跑。
3. 和規格或任務檔不一樣的地方，以及原因。
4. 寫進 `docs/decisions.md` 的待決定事項。
5. 建議複查的檔案和行數範圍。
