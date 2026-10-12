# M5 · bench：量測、校準與優先權實驗

> 規格依據：v0.4 §3（優先權例外與實驗規則）、§11、§12B、§12C
> 前置：M3　｜　環境：統計與解析在雲端 Linux，量測與擷取在本機 Windows　｜　建議 effort：中到高　｜　完成後複查：建議（統計方法）

## 目標

建立一套可以重複執行的量測工具，回答兩個問題：

1. 我們這端準不準：Hook 到 SendInput 的延遲分布、晚醒、停止耗時。
2. 遊戲吃不吃得到：每種動作的按住時間與間距，在什麼範圍內能穩定成功。

輸出是一份帶身分資訊的 `TimingProfile`，通過預先定好的驗收門檻才標成 `verified`。

## 安全提醒

校準只在練習關卡這類非排名內容進行，不要在排名戰執行。

## 範圍

### 要做
- `tools/bench/core`（純邏輯，Linux 可測）：讀環形紀錄 CSV、讀 PresentMon CSV、時間對齊、統計、信賴區間、掃描規劃、報告產生。
- `tools/bench/win`（Windows）：DXGI Desktop Duplication 擷取與變化判定、啟動 PresentMon、透過控制程序下校準指令、執行掃描。
- IPC 新增 `RunTrial`，`Arm` 加上 `mode: Normal | Calibration` 欄位（`RunTrial` 只允許在 calibration session）。
- 偵測器驗證流程：匯出截圖給人工標註，比對誤判率。
- 優先權與尾段自旋的 A/B 實驗工具。
- 報告：CSV＋Markdown。

### 不做
- GUI 上的報告檢視（M6，只需要能開啟這裡產生的檔案）。

## 時間對齊

所有來源都用 QPC：

| 來源 | 欄位 |
|---|---|
| 環形紀錄 | `hook_enter`、`enqueue`、`engine_receive`、`deadline`、`send_begin`、`send_end` |
| PresentMon（加 `--qpc_time`） | `CPUStartQPC`、`MsBetweenPresents`、`MsUntilDisplayed` |
| DXGI | `DXGI_OUTDUPL_FRAME_INFO.LastPresentTime` |

衍生數字（和時序圖頁面的定義一致）：鉤子處理 = enqueue − hook_enter；喚醒延遲 = engine_receive − enqueue；晚醒 = send_begin − deadline；注入耗時 = send_end − send_begin；落在哪一幀 = send_end 之後第一個 CPUStartQPC；輸入到畫面 = 偵測到變化的 LastPresentTime − send_end。

## 軟體路徑量測（§12B）

- 報告 P50、P95、P99、P99.9、最大值、樣本數、遺失筆數。樣本數不足時不顯示高分位：P99 至少 1,000 筆，P99.9 至少 10,000 筆。
- 負載情境：遊戲閒置、GPU 高負載、CPU 高負載、多鍵交錯、頻繁切換焦點、移動視窗、GUI 持續操作、換設定、紀錄讀取端落後、輸入洪流、睡眠後恢復。每個情境一份結果。
- 停止耗時：`stop_request → stop_ack` 的分布。

## 遊戲校準（§12C）

### 先寫計畫，再跑

每次校準先產生一份計畫檔 `calib/plan-<時間>.json`，跑之前算出雜湊並寫進結果。計畫包含：

- 用哪一個設定檔（六人或十人）、手牌在哪一邊。
- 動作種類與要掃的參數（`key_hold`、`mouse_hold`、`key_to_click`、`repeat_period`、`escape_gap`、`card_pre_esc`）。牌鍵是一次按鍵脈衝，主要掃 `key_hold`。
- 粗掃網格、細掃步距、每個點的次數。
- 成功判定：偵測器種類、畫面區域、門檻。
- 驗收門檻：成功率信賴區間下界要達到多少、信賴水準。
- 負載條件與遊戲設定（幀率、視窗模式、解析度）。

六人和十人要分開校準：十人的制約解除決戰節奏比較快，結果寫回各自設定檔的 `timing`。校準對象是規格 §5 的七組間隔；每次校準指定要寫回哪一組（例如只寫 `frame_slow`），其他組不動。

結果檔裡的門檻必須和計畫一致；跑完才改門檻的結果不能標成 verified。

### 試驗方法

- 每次試驗前加一段隨機延遲（0 到 1 幀之間均勻分布），讓起始相位隨機。原因見時序圖頁面「間距和幀」那一節。
- 先粗掃、再在成功率變化的邊界細掃。**不假設單調**：卡幀類動作可能同時有最小和最大間距，有效範圍可能不只一段。
- 每次試驗記錄：有效、漏操作、誤操作、順序錯、重複；遊戲條件不符（技能冷卻、資源不足）另外標記，不算失敗。
- 單一動作通過後，再測多個 repeat、牌鍵、普通點位與停止交錯。
- 牌鍵另外量：游標在每一格中心、格與格的縫、手牌邊緣時，送出的鍵是否正確（這一項只看我們這端的判定，可以用接收器量，不需要遊戲）。

### 統計

- 成功率用 Wilson 信賴區間，報告樣本數和區間。
- 次數提示：零失敗時，95% 信心下失敗率上界約為 3 ÷ n。要宣稱成功率 ≥ 99.5%，至少要約 600 次零失敗。計畫工具要依門檻自動算出最少次數。
- 選值：在可靠範圍內留餘裕，不採用偶爾成功的最快點。餘裕至少涵蓋第 1 層量到的 P99.9 晚醒。

### 偵測器

- DXGI 擷取遊戲所在的螢幕，只看計畫指定的區域。基本偵測器：和基準畫面的平均差異超過門檻；可以加顏色條件。
- 需要視窗或無邊框模式。擷取在控制程序或 bench 程序裡做，不進引擎程序。
- 驗證：每種偵測設定先匯出一批試驗前後的截圖和 CSV，讓使用者人工標註，算出偵測器和人工的混淆矩陣。誤判率低於計畫門檻才能用來大量掃描。必要時用 240fps 錄影當最終對照。

### 輸出

`TimingProfile`，寫回校準時用的那個設定檔的 `timing`（透過 M4 的 `config::store` 原子存檔，改之前先備份）：
- `identity`：遊戲執行檔版本、視窗模式、PresentMon 量到的幀率中位數、螢幕更新率、解析度、機器識別、負載條件、模式（六人／十人）。
- `source: Calibrated`；全部驗收通過才 `verified: true`。
- 校準期間如果高解析度計時器不可用（共享記憶體的 `degraded_flags`），這份 profile 不能標成 verified。
- 報告附上每個參數的成功率曲線（CSV）和選值理由。

## `RunTrial`

```rust
AppToEngine::RunTrial { cmd_seq: u64, session: SessionId, trial: TrialSpec }
// TrialSpec：要執行的動作、覆寫的時間參數、開始時間（QPC）
```

- 只有用 `Arm { mode: Calibration }` 開啟的 session 可以執行；一般 session 收到就拒絕。
- 不經過 Hook，但照樣走 Engine、Driver、帳本與紀錄，停止協定照常有效。
- 每筆紀錄標上 trial 編號。

## 優先權與自旋實驗（§3）

- 同一組負載下切換：Hook 一般優先權 vs TIME_CRITICAL；Engine 一般 vs HIGHEST；尾段自旋關 vs 開（長度可設）。
- 比較：`hook_enter` 延遲分布、Hook 逾時或失聯次數、晚醒分布、遊戲幀時間（PresentMon）、CPU 使用率、停止耗時。
- 判定規則：只有尾延遲改善，而且遊戲幀時間、CPU、停止耗時都沒有變差，才保留。結果寫進 `docs/decisions.md`。
- Hook 的 TIME_CRITICAL 是預設值；如果實驗顯示它讓遊戲幀時間變差，改回一般並記錄。

## 測試與驗收

**Linux（雲端）**

- [ ] 環形紀錄 CSV 與 PresentMon CSV 解析，含缺欄位、遺失筆數。
- [ ] 時間對齊：給定一組 send_end 和幀開始時間，算出的「落在哪一幀」正確。
- [ ] 分位數與「樣本不足不顯示」規則。
- [ ] Wilson 區間：和已知數值比對；次數計算（零失敗、99.5%、95% 信心 → 約 600）。
- [ ] 掃描規劃：給定一條非單調的模擬成功率曲線（例如中間一段有效、兩邊失敗），能找出整段有效範圍，不會被二分搜尋誤導。
- [ ] 計畫雜湊：門檻和計畫不一致時，結果不能標成 verified。
- [ ] 用 `core::sim::FakeGame` 產生合成資料（M1 沒做的話在這裡補上），整條分析流程跑得通。

**Windows（本機）**

- [ ] 以 M2 接收器當目標，跑完一次軟體路徑量測，產生報告。
- [ ] DXGI 偵測器在接收器上驗證：接收器收到按鍵時畫面會變色，偵測時間和接收器的 QPC 紀錄一致。
- [ ] PresentMon 有開 `--qpc_time` 時，時間能和環形紀錄對齊。
- [ ] A/B 實驗工具能跑完一輪並輸出比較表。

**遊戲（使用者，非排名內容）**

- [ ] 至少完成一種動作（建議先做卡幀暫停）的完整校準，產生 verified 的 timing profile。

## 複查清單（建議）

- 計畫和結果的對應是否防得住「跑完再改門檻」？
- 隨機相位是否真的均勻、是否記錄了每次的相位？
- 非單調的成功率曲線是否處理正確？
- 偵測器驗證是否在大量掃描之前完成？
- `RunTrial` 是否只能在 calibration session 執行？

## 貼給 Codex 的開場訊息

```text
請完成里程碑 M5。先讀 AGENTS.md、docs/spec/BAauto-rs-融合架構-v0.4.md（重點 §3、§11、§12）和 docs/tasks/M5-bench.md。M1–M3 已完成。

這一步要做：tools/bench/core（純邏輯的解析、對齊、統計、掃描規劃、報告）、tools/bench/win（DXGI 擷取與偵測、PresentMon、執行掃描）、IPC 的 RunTrial、偵測器驗證流程，以及優先權與自旋的 A/B 實驗工具。

做法：先把 tools/bench/core 在 Linux 上做完並通過測試，特別是非單調曲線和 Wilson 區間。Windows 部分讓 cargo check --target x86_64-pc-windows-gnu 通過，並寫一份 docs/bench-howto.md，一步一步說明我要怎麼在自己的電腦上跑量測和校準。完成後照 AGENTS.md 的格式回報。
```
