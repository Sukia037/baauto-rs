# M1 · core：純邏輯核心

> 規格依據：v0.4 §4、§5、§7、§9、§9A、§10（點位 JSON 與預設按鍵 JSON 格式）、§11、§12A
> 前置：無　｜　環境：雲端 Linux 可完成　｜　建議 effort：高　｜　完成後複查：建議

## 目標

做出不依賴 Windows、可以用假時鐘完整測試的動作引擎和設定編譯器。之後的 Windows 層只是把真的時間、真的輸入接到這裡。M1 結束時，規格裡所有「動作順序」的保證都要能在 Linux 上用測試證明。

## 範圍

### 要做
- Cargo workspace 骨架（照 `AGENTS.md` 的結構建空的 crate，之後的里程碑再填）。
- `crates/config`：兩種檔的模型——點位檔（`baauto-rs-points-v1`：六人／十人＋點位按鍵清單，功能是普通點位或牌鍵）和預設按鍵檔（`baauto-rs-keys-v1`：23 個預設按鍵＋時間）；加上 `settings.json` 裡的手牌設定 `HandSettings`；`TimingProfile`、熱鍵 `Chord` 與可讀字串格式、衝突分析、驗證器；「點位檔＋預設按鍵檔＋手牌設定」一起編譯成 `CompiledConfig`。
- `crates/core`：時間與 ID 型別、輸入事件、引擎狀態機、六種動作（含牌鍵）、手牌格判定、資源仲裁、停止協定、健康期限。
- 範本：`points/範例.json`（六人，兩個普通點位、一個牌鍵）、`keys/預設按鍵.json`（AHK 版的 23 個預設熱鍵、全部啟用），以及 `HandSettings::default()`；範例點位檔設成六人、十人各編譯一次都要成功。
- `crates/core` 的模擬器 `sim`：假時鐘、假 Driver（可以注入晚醒和短送）、紀錄每一批的預計與實際時間。
- 單元測試、情境測試、proptest 不變量、穩態零配置測試。

### 不做
- 任何 Windows API、執行緒、佇列、共享記憶體（M2）。
- 舊 AHK／MuMu 檔案匯入、座標模式轉換（M4）。座標一律是遊戲畫面（全螢幕 16:9 的整個螢幕）的 0–1 座標。
- GUI。

## 介面草案

名稱可以調整，語意不能變。

```rust
// ---- core::time / ids ----
pub struct Ticks(pub u64);            // 單調時間；Windows 上就是 QPC 值
pub struct TickRate(pub u64);         // 每秒幾個 tick
pub struct SessionId(pub u32);
pub struct PressId(pub u32);          // 每次有效按下一個，由 Hook 產生
pub struct ActionId { pub slot: u16, pub generation: u32 }  // 槽位重用要換 generation
pub struct ConfigVersion(pub u32);
pub struct GeometryVersion(pub u32);
pub struct NormPoint { pub x: f32, pub y: f32 }   // 0–1，相對遊戲畫面（遊戲全螢幕所在的螢幕）

// ---- 輸入（M2 的 Hook／平台層會產生這些）----
pub enum InputEvent {
    TriggerDown   { seq: u64, at: Ticks, session: SessionId, press: PressId,
                    trigger: TriggerIdx, cursor: NormPoint, geometry: GeometryVersion },
                    // cursor：按下那一刻的游標位置，牌鍵用它判定手牌格
    TriggerRepeat { seq: u64, at: Ticks, session: SessionId, press: PressId },   // OS 自動重複
    TriggerUp     { seq: u64, at: Ticks, session: SessionId, press: PressId },
    PressCancelled{ seq: u64, at: Ticks, session: SessionId, press: PressId, reason: CancelReason },
    Focus         { seq: u64, at: Ticks, focused: bool, geometry: GeometryVersion },
    PhysicalConflict { seq: u64, at: Ticks, key: OutKey },  // 使用者實體按著我們正在合成的鍵
}

pub enum StopReason { User, Emergency, FocusLost, GeometryChanged, Overflow, SeqGap,
                      ConfigChange, PhysicalConflict, SupervisorLost, Fault(FaultKind) }

// ---- 輸出 ----
pub enum OutKey { Key(Key), LeftButton }
pub enum OutEvent { KeyDown(Key), KeyUp(Key), MoveTo(NormPoint), LeftDown, LeftUp }
pub struct Batch {
    pub id: u64,
    pub session: SessionId,
    pub action: Option<ActionId>,      // 停止時的釋放批次為 None
    pub deadline: Ticks,               // 預計時間（紀錄的 deadline 欄位）
    pub events: ArrayVec<OutEvent, 4>,
}
pub enum SubmitOutcome {
    Ok    { begin: Ticks, end: Ticks },
    Short { inserted: u8, end: Ticks },   // 少送或零送
}

// ---- 引擎 ----
pub enum EngineState { Disarmed, Armed(SessionId), Running(SessionId),
                       Stopping(SessionId, StopReason), Stopped(SessionId), Faulted(SessionId, FaultKind) }

impl Engine {
    pub fn new(cfg: Arc<CompiledConfig>, rate: TickRate) -> Self;   // 只有這裡可以配置記憶體
    pub fn arm(&mut self, session: SessionId, now: Ticks) -> Result<(), ArmError>;
    pub fn on_input(&mut self, ev: InputEvent, now: Ticks);
    pub fn request_stop(&mut self, reason: StopReason, now: Ticks);
    pub fn next_batch(&mut self, now: Ticks) -> Option<Batch>;      // 一次最多一批
    pub fn commit(&mut self, batch_id: u64, outcome: SubmitOutcome, now: Ticks);
    pub fn next_wake(&self) -> Option<Ticks>;     // 最近的動作期限和健康期限取早者
    pub fn state(&self) -> EngineState;
    pub fn held(&self) -> HeldSet;                // 已成功送出 Down、還沒確認 Up 的鍵
    pub fn drain_reports(&mut self, out: &mut impl FnMut(Report));  // 拒絕、略過、取消、需要人工確認
    pub fn progress(&self) -> Progress;           // scheduler_progress、action_progress
}
```

兩階段送出：`next_batch` 只產生批次，**`commit` 收到實際結果後**才推進動作階段、才用實際的 `end` 計算下一個期限。這是「每階段從實際提交時間算最小間隔」的實作方式。

## 設定（`crates/config`）

```rust
pub struct Chord { pub key: Key, pub mods: ModsMatch }
pub enum ModsMatch {
    AtLeast(Mods),   // 這些修飾鍵要按著，多按其他修飾鍵也觸發。AHK 的 `*` 前綴就是這個語意；
                     // AHK 版所有熱鍵都用 `$*` 註冊，所以預設是 AtLeast
    Exact(Mods),     // 修飾鍵必須完全一致
}

pub enum Mode { Six, Ten }                 // 手牌 3 格／5 格
pub enum CardUiSide { Right, Left }        // 遊戲選項「スキルカードUI位置」，預設 Right
pub enum SlotOrder { LeftToRight, RightToLeft }
pub struct HandRect { pub x: (f32, f32), pub y: (f32, f32), pub calibrated: bool }  // 遊戲畫面 0–1 座標

pub struct PointSet {                      // 點位 JSON（schema baauto-rs-points-v1），一個關卡或 Boss 一份
    pub name: String,
    pub mode: Mode,                        // 六人／十人，跟著點位檔走
    pub items: Vec<PointItem>,             // 點位按鍵清單
}
pub struct PointItem { pub id: PointId, pub name: String, pub hotkey: Option<Chord>, pub function: PointFunction }
pub enum PointFunction {
    Point { coord: Coord },                // 普通點位：按住時游標移到點位並按住左鍵；Coord 是遊戲畫面 0–1 座標
    Card,                                  // 牌鍵：看游標在手牌第幾格，送那一格的按鍵
}

pub struct KeysProfile {                   // 預設按鍵 JSON（schema baauto-rs-keys-v1），和 AHK 版 function_keys.json 同一個角色
    pub presets: PresetTable,              // 23 個預設功能的熱鍵與啟用狀態（不分六人／十人）
    pub timing: TimingProfile,
}

pub struct HandSettings {                  // settings.json 裡這台電腦的手牌設定，所有點位檔共用
    pub card_ui_side: CardUiSide,
    pub slot_order: SlotOrder,
    pub rects: HandRects,                  // 六人右、六人左、十人右、十人左四組 HandRect
    pub slot_keys: [Key; 5],               // 六人用前 3 個
    pub card_auto_repeat: AutoRepeatPolicy,// 牌鍵按住時是否跟著自動重複（預設 Ignore）
}

pub struct TimingProfile {
    pub identity: ProfileIdentity,   // 遊戲版本、視窗模式、FPS／同步設定、機器、測試條件
    pub source: ProfileSource,       // ImportedLegacy | Manual | Calibrated
    pub verified: bool,              // identity 任一欄位改變就要變回 false
    pub key_hold_us: u32,
    pub mouse_hold_us: u32,
    pub key_to_click_us: u32,
    pub repeat_period_us: u32,
    pub escape_gap_us: Range<u32>,   // 雙 Esc 第二下可接受的區間（有最小也有最大）
    pub card_pre_esc_us: u32,
    pub hold_maintain_period_us: u32,
    pub allowed_lateness_us: LatenessTable, // 每種動作、每個階段過期多少要取消
    pub card_trailing_esc: TrailingEscPolicy,
    pub auto_repeat: AutoRepeatPolicy,      // Ignore | FollowTypematic（AHK 原本的行為）
}

pub struct EngineParams {   // 工程調校，和遊戲時序分開
    pub max_actions: u16, pub queue_capacity: u32, pub max_events_per_round: u16,
    pub max_batches_per_round: u16, pub health_period_us: u32,
}

pub fn compile(points: &PointSet, keys: &KeysProfile, hand: &HandSettings, rate: TickRate) -> Result<CompiledConfig, Vec<ConfigError>>;
pub fn slot_at(layout: &HandRect, slots: u8, order: SlotOrder, p: NormPoint) -> Option<u8>;  // 純函式
```

**兩種 JSON（使用者會手改，格式要好讀）**

點位檔 `points/範例.json`：

```json
{
  "schema": "baauto-rs-points-v1",
  "name": "範例",
  "mode": "six",
  "items": [
    { "id": 1, "name": "Boss 中央", "hotkey": "z", "function": "point", "x": 0.5, "y": 0.389 },
    { "id": 2, "name": "左側集火", "hotkey": "x", "function": "point", "x": 0.297, "y": 0.569 },
    { "id": 3, "name": "出牌", "hotkey": "q", "function": "card" }
  ]
}
```

（普通點位的 `x`、`y` 是遊戲畫面上的 0–1 座標。遊戲一律全螢幕 16:9，所以所有解析度共用，換解析度不用重錄；介面顯示時換成目前解析度的像素。）

預設按鍵檔 `keys/預設按鍵.json`：

```json
{
  "schema": "baauto-rs-keys-v1",
  "presets": {
    "skill_1": { "hotkey": "w", "enabled": true },
    "skill_4": { "hotkey": "t", "enabled": true },
    "frame_fast": { "hotkey": "Down", "enabled": true }
  },
  "timing": {
    "source": "manual", "verified": false,
    "key_hold_ms": 16, "mouse_hold_ms": 16, "key_to_click_ms": 16, "repeat_period_ms": 50,
    "escape_gap_ms": [16, 50], "card_pre_esc_ms": 10, "hold_maintain_period_ms": 16
  }
}
```

`settings.json` 裡的手牌設定（M4 負責整個 `settings.json`，這裡只定義 `HandSettings` 和預設值）：

```json
"hand": {
  "card_ui_side": "right",
  "slot_order": "left_to_right",
  "slot_keys": ["1", "2", "3", "4", "5"],
  "card_auto_repeat": "ignore",
  "rects": {
    "six": { "right": { "x": [0.664, 0.910], "y": [0.781, 0.924], "calibrated": false },
             "left":  { "x": [0.090, 0.336], "y": [0.781, 0.924], "calibrated": false } },
    "ten": { "right": { "x": [0.500, 0.910], "y": [0.781, 0.924], "calibrated": false },
             "left":  { "x": [0.090, 0.500], "y": [0.781, 0.924], "calibrated": false } }
  }
}
```

（`presets` 範例只列三項，實際檔案 23 項都要有。時間數字只是範例，不是建議值。）

- 時間在 JSON 裡用毫秒（可以有小數），讀進來換成 µs 再換成 tick。
- 熱鍵用可讀字串：`w`、`Down`、`XButton1`、`Ctrl+F9`、`Shift+RButton`。預設是 `AtLeast`（多按其他修飾鍵也觸發，和 AHK 版一樣）；前面加 `=` 表示修飾鍵必須完全一致，例如 `=Ctrl+F9`。
- 讀到未知欄位、型別錯誤、`slot_keys` 長度不對：回報清楚的 JSON 路徑和原因，不 panic、不默默忽略。
- 範本和預設值：預設按鍵檔 23 個全部啟用（不分六人／十人）；手牌四組矩形都標 `calibrated: false`。

**預設功能**（和 AHK 版一樣的 23 個，`global_stop`、`record` 由監督程序處理，不進引擎）：

| role | 預設熱鍵 | 動作 |
|---|---|---|
| global_stop／record | Ctrl+F9／Ctrl+F10 | 監督程序的全域熱鍵 |
| skill_1–5 | w e r t y | Tap 1–5 |
| spam_1–5 | 2 3 4 5 6 | Repeat（技能鍵＋點擊）1–5 |
| pause | Space | Tap Esc（佔用 Esc 資源） |
| frame_fast／frame_slow | Down／Up | DoubleEsc |
| auto／speed | XButton1／XButton2 | Tap a／Tap s |
| click_spam | RButton | Repeat（只點擊） |
| card_1–5 | a s d f g | Card 1–5 |

**編譯規則**
- 熱鍵正規化成 `Chord`。預設功能彼此衝突：雙方都停用，而且預設按鍵檔不能存。點位檔的項目（普通點位或牌鍵）撞到預設功能或彼此重複：該項停用並回報原因，點位檔照樣可以存。（和 AHK 版相同）
- 拒絕把左鍵設成任何觸發鍵。
- `slot_keys` 必須有 5 個、不能重複；六人模式只用前 3 個。
- 手牌矩形必須在 0–1 之內、寬高大於 0。編譯時用點位檔的 `mode` 和 `card_ui_side` 選四組裡的一組。
- 驗證器：每種動作的週期不能短於它各階段時間的總和。例如 Repeat 的 `repeat_period` ≥ `key_hold + key_to_click + mouse_hold`。
- `key_hold_us = 0` 只有在 `verified = true` 時才允許；否則 Tap 一定分成 Down、Up 兩批。
- 編譯結果包含：固定大小的觸發表（每個 key 一格）、動作規格陣列、`ConfigVersion`、所有時間換算成 tick。

## 行為規格摘要

細節以規格為準，這裡列實作時最容易漏的。

**狀態機**：Disarmed → Armed（明確 Arm）→ Running（有動作）→ Armed（動作都結束）。任何停止原因 → Stopping → Stopped，釋放失敗 → Faulted。Faulted 只能回到 Disarmed，再由新的 Arm 啟用。

**事件身分**：所有事件要核對 session。KeyUp 用 `press` 找到原本那個 action，不重新用目前設定解釋。快速放開再按下會是新的 `press`，舊的期限不能影響新動作。輸入事件的 `seq` 必須連續，出現缺口就 `request_stop(SeqGap)`。

**失焦**：`Focus { focused: false }` → `request_stop(FocusLost)`，這個 session 就此結束。焦點回來之後要不要自動開新 session，是監督程序的事（M3），引擎不自己恢復。

**動作**

| 動作 | 階段與時間 | 資源 |
|---|---|---|
| Tap | KeyDown →`key_hold`→ KeyUp | 無（Esc 要 Esc 資源） |
| Repeat | 〔KeyDown →`key_hold`→ KeyUp →`key_to_click`→〕LeftDown →`mouse_hold`→ LeftUp；下一輪開始 = max(本輪開始＋`repeat_period`, 上一批 end＋最小間隔) | 有點擊時要滑鼠資源，**整輪**取得後才送技能鍵 |
| Card | Esc 脈衝 →`card_pre_esc`→ 重複（技能鍵＋點擊）直到放開；正常放開時依 `card_trailing_esc` 決定補不補 Esc | Esc 資源＋滑鼠資源 |
| DoubleEsc | Esc 脈衝 →`escape_gap`→ Esc 脈衝；第二下如果會晚過 `escape_gap.end`＋允許延遲，就取消並回報，不盲目補送 | Esc 資源 |
| HoldPoint | MoveTo＋LeftDown（同一批）→ 每 `hold_maintain_period` MoveTo → 放開時 LeftUp | 滑鼠資源，獨占 |
| CardKey（牌鍵） | 用 `TriggerDown.cursor` 和 `slot_at` 判定第 i 格 → 送 `slot_keys[i]` 的 Tap；不在手牌上就不送並回報 | 只佔該格按鍵本身 |

- 牌鍵只在按下那一刻判定，之後游標怎麼動都不影響。放開熱鍵時，如果 Tap 還沒送完（例如 `key_hold` 還沒到），照樣把 Up 送完，不中斷成半個脈衝。
- 衝突的新請求直接拒絕並回報原因，不延後執行：普通點位持有中再按另一個普通點位、Esc 資源被佔用時按 Card／DoubleEsc／pause、牌鍵要送的鍵正被其他動作按住。
- 純鍵盤動作（Tap、牌鍵）可以和普通點位同時進行，除非是同一個鍵或需要 Esc 資源。
- Repeat 過期的週期直接跳過並計數，永遠不一次補送多輪。
- 修飾鍵放開造成 chord 失效（`PressCancelled`）：取消該 press 的動作。
- `AutoRepeatPolicy::FollowTypematic` 時，`TriggerRepeat` 對 Tap 再觸發一次；`Ignore` 時忽略。牌鍵看 `HandSettings.card_auto_repeat`（預設 Ignore）。Repeat、HoldPoint 等持續型動作一律忽略 `TriggerRepeat`。
- `geometry` 變了就取消普通點位（持有左鍵時送 LeftUp）。

**手牌格判定 `slot_at`**：手牌矩形依格數切成等寬的格；格與格之間的縫算進最近的格，沒有死區；矩形外回傳 `None`。`slot_order` 為 `RightToLeft` 時把編號反過來。剛好落在兩格分界線上，算右邊那一格（寫成測試，固定下來）。

**停止**：進 Stopping 後不再開始任何新動作、所有期限失效，產生一個釋放批次（所有 `held` 的 KeyUp，持有左鍵時加 LeftUp）。Card 在停止或失焦時**不**補 Esc，改回報「需要人工確認暫停狀態」。釋放批次 commit 成功才進 Stopped 並清空 `held`；短送就進 Faulted，`held` 保留沒確認的鍵。

**健康**：`next_wake` 一定包含健康期限（`health_period`），完全閒置時也會醒來推進 `scheduler_progress`。

**有界工作**：`next_batch` 每次只回傳期限最早、已到期的一批；同時到期依 ActionId 排序。`on_input`、`next_batch`、`commit` 在穩態不配置記憶體。

## 模擬器 `core::sim`

給測試和之後的重播用：

- `FakeClock`：手動前進。
- `FakeDriver`：收批次、照劇本回傳 `SubmitOutcome`，可以設定「第 N 批晚 X tick」「第 N 批短送」。
- `FakeGame`（選配）：用固定幀時間和相位，算出每個輸出在哪一幀被讀到，給 M5 和時序圖對照用。
- 每一批記下 `deadline`、`begin`、`end`、`action`、`stage`，可以輸出成 CSV。

## 測試與驗收

**情境測試**（名稱照用）

- [ ] `repeat_late_wake_shifts_click_stage`：連點的技能鍵 Up 晚送 3ms → 點擊的期限 = 技能鍵 Up 的 end＋`key_to_click`，間隔不縮短。
- [ ] `repeat_skips_overdue_cycles_without_burst`：引擎卡住 5 個週期後恢復，恢復後第一個 `repeat_period` 內只送一輪，略過計數 ≥ 4，沒有連續補送。
- [ ] `quick_release_repress_uses_new_press`：放開後 1 tick 內重按，舊 press 的期限不會讓新動作提早或多送。
- [ ] `stop_releases_everything_then_silent`：任何狀態下停止 → 一個釋放批次 → Stopped → 之後同一 session 不再有任何批次。
- [ ] `short_send_goes_faulted_and_keeps_held`：釋放批次短送 → Faulted，`held` 沒有被清成空的。
- [ ] `double_esc_cancels_when_beyond_max_gap`：第二下會超出 `escape_gap.end`＋允許延遲 → 不送第二下，回報取消。
- [ ] `card_no_trailing_esc_on_focus_loss`：Card 進行中失焦 → 釋放按鍵，但不送結尾的 Esc，回報需要人工確認。
- [ ] `hold_point_rejected_while_another_hold_point_active`。
- [ ] `esc_resource_is_exclusive`：DoubleEsc 進行中按 Card 或 pause → 拒絕。
- [ ] `repeat_with_click_takes_mouse_for_whole_cycle`：滑鼠被普通點位佔用時，Repeat 不會先送技能鍵。
- [ ] `modifier_release_cancels_press`。
- [ ] `auto_repeat_policy_ignore_vs_follow`。
- [ ] `geometry_change_cancels_hold_point_and_releases_left`。

**牌鍵**

- [ ] `card_key_sends_key_of_slot_under_cursor`：六人 3 格、十人 5 格，每一格中心各按一次，送出的鍵正確。
- [ ] `card_key_outside_hand_sends_nothing_and_reports`：手牌矩形外（上下左右各一點）不送任何鍵，回報「游標不在手牌上」。
- [ ] `card_key_seam_and_boundary`：格與格的縫、分界線、矩形邊緣的判定和規則一致。
- [ ] `card_key_left_side_uses_left_layout`、`card_key_slot_order_right_to_left`。
- [ ] `card_key_ignores_auto_repeat_by_default`、`card_key_follow_typematic_when_configured`。
- [ ] `card_key_rejected_when_slot_key_held`：該格的鍵正被 Repeat 或另一個 Tap 按住 → 拒絕並回報。
- [ ] `card_key_completes_pulse_after_release`：熱鍵在 `key_hold` 之內放開，Up 照樣送出，不殘留。
- [ ] `card_key_runs_alongside_hold_point`：普通點位持有滑鼠時，牌鍵照常送出。

**設定**

- [ ] `config_rejects_left_button_trigger`、`config_preset_conflict_blocks_save`、`config_point_hotkey_conflict_disables_point`、`config_cycle_shorter_than_stages_rejected`。
- [ ] `hand_slot_keys_need_five_unique`、`config_hand_rect_must_be_valid`、`mode_picks_hand_rect_and_slot_count`。
- [ ] `config_templates_load_and_compile`：`points/範例.json`（設成六人、十人各一次）配 `keys/預設按鍵.json` 和預設 `HandSettings`，讀得進來、編譯成功。
- [ ] `points_card_item_needs_no_coord`、`points_item_conflict_disables_item_but_file_saves`、`switching_keys_file_keeps_points_file`（換預設按鍵檔不影響點位檔，反之亦然）。
- [ ] `config_reports_json_path_on_error`：未知欄位、型別錯誤時，錯誤訊息帶 JSON 路徑。
- [ ] `config_hotkey_string_roundtrip`：`w`、`Down`、`XButton1`、`Ctrl+F9`、`=Ctrl+F9` 解析後再輸出，字串相同。
- [ ] `idle_engine_still_advances_health`：沒有任何動作時，`next_wake` 仍然在 `health_period` 內。
- [ ] `stale_session_events_are_ignored`：舊 session 的事件和 commit 不會影響新 session。
- [ ] `focus_loss_stops_session`、`seq_gap_stops_session`。
- [ ] `tap_zero_hold_requires_verified_profile`。

**proptest 不變量**：隨機產生事件序列（按下、放開、重複、取消、失焦、幾何變更、停止）、隨機晚醒、隨機短送，每一步檢查：

- [ ] 每個鍵的合成持有數只會是 0 或 1，Down、Up 交替出現。
- [ ] 任何時刻最多一個動作持有滑鼠資源；Esc 資源同理。
- [ ] Stopped 之後，該 session 沒有任何新批次。
- [ ] 成功停止後 `held` 為空；Faulted 時 `held` 不為空或有記錄原因。
- [ ] 同一個動作相鄰兩批的開始時間，不小於前一批 end 加上設定的最小間隔。
- [ ] 任意長度 W 的時間窗內，一個 Repeat 的輸出輪數 ≤ ⌈W ÷ repeat_period⌉＋1。
- [ ] `next_wake` 永遠不是 `None`（除了 Disarmed／Faulted）。

**其他**

- [ ] 穩態零配置：用計數型 `#[global_allocator]` 的整合測試，Arm 之後跑 10 萬步隨機事件，`on_input`／`next_batch`／`commit` 期間配置次數為 0。
- [ ] `core`、`config` 都是 `#![forbid(unsafe_code)]`，`cargo test` 在 Linux 全過。
- [ ] `sim` 能把一個情境輸出成 CSV，欄位：`batch_id, action, stage, deadline, begin, end`。

## 複查清單（給 Astra／Opus）

- 兩階段送出：有沒有任何地方在 commit 之前就推進階段或改期限？
- 停止：Stopping 期間有沒有路徑還會產生非釋放的批次？短送時 `held` 是否正確保留？
- 資源：Repeat 的技能鍵＋點擊是否整輪取得；普通點位持有滑鼠時，牌鍵與 Tap 是否照常放行、Esc 是否照樣互斥？
- 牌鍵：`slot_at` 的邊界規則是否和測試一致；左右版面、`slot_order` 是否在編譯時就決定好，不在熱路徑上判斷字串？
- 時間：有沒有用到時間單位不一致（µs、tick 混用）或整數溢位的地方？
- proptest 的產生器有沒有涵蓋「停止和按下同一 tick」「commit 晚到」這些邊界？

## 貼給 Codex 的開場訊息

```text
請完成里程碑 M1。先讀 AGENTS.md、docs/spec/BAauto-rs-融合架構-v0.4.md（重點 §4、§5、§7、§9、§11、§12A）和 docs/tasks/M1-core.md。

這一步只做純邏輯：crates/config 與 crates/core，加上 core::sim 模擬器、points/ 的範例點位檔和 keys/ 的範本預設按鍵檔。設定分成點位 JSON（含六人／十人）和預設按鍵 JSON 兩種檔，手牌設定在 settings.json（任務檔「兩種 JSON」），不要合成一個，也不要搬動欄位。不要碰任何 Windows API、執行緒或共享記憶體。注意 v0.4 已經拿掉拖曳，改成牌鍵（規格 §9A），並加入六人／十人模式與手牌左右位置。

做法：
1. 先建 workspace 骨架和所有 crate 的空殼。
2. 先把任務檔「測試與驗收」列的測試寫出來（可以先 #[ignore] 或 todo!()），給我看測試清單和公開介面。
3. 再實作到全部通過：cargo fmt --check、cargo clippy --all-targets -- -D warnings、cargo test。

規格沒講清楚的地方，寫進 docs/decisions.md，選一個保守的做法繼續。完成後照 AGENTS.md 的格式回報。
```
