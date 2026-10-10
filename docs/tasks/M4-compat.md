# M4 · compat：舊設定匯入、點位檔與預設按鍵檔管理、存檔、功能對照

> 規格依據：v0.4 §9、§9A、§10、§11（舊時序值的處理）
> 前置：M1（可以和 M2、M3 同時進行）　｜　環境：雲端 Linux 可完成（存檔的 Windows 替換 API 除外）　｜　建議 effort：中　｜　完成後複查：抽查

## 目標

讓使用者手上所有舊檔案都能安全地轉成新格式（M1 定義的兩種 JSON）：AHK 版的功能按鍵 JSON → 預設按鍵 JSON（`baauto-rs-keys-v1`），AHK 版的點位 JSON → 點位 JSON（`baauto-rs-points-v1`，拖曳點位變成功能為牌鍵的項目）；更舊的 `keys` 格式、MuMu 模擬器的按鍵設定也能匯入。和 AHK 版一樣，兩種檔分開，各自切換、各自存檔。同時做好檔案管理（`points/`、`keys/` 資料夾與 `settings.json`，含手牌設定）、原子存檔，以及新舊行為對照表。

## 前置資料

- `reference/BAauto.ahk`：原始腳本，對照匯入與行為用。
- `tests/fixtures/legacy/`：使用者提供的真實檔案。如果資料夾是空的，先用下面「合成測試檔」繼續，並在回報裡提醒使用者補檔。

## 範圍

### 要做
- `crates/config::legacy`：舊格式匯入器，全部是純函式。輸出一個 `KeysProfile`（從功能 JSON）或 `PointSet`（從點位 JSON、`keys` 格式），加一份匯入報告。
- AHK 熱鍵字串解析器：轉成 M1 的 `Chord`，存檔時用 M1 的可讀字串格式（例如 `Ctrl+F9`）。
- 點位資料整合：AHK 的 points／bindings 兩張表合成點位檔的 `items`，一筆一個按鍵綁定；ID 重新編號、孤兒清理。
- 座標：遊戲一律全螢幕 16:9，舊檔的 0–1 座標（相對整個鎖定螢幕）就是新版的遊戲畫面座標，直接沿用，不需要換算。
- `crates/config::editor`：點位檔、預設按鍵檔與手牌設定編輯操作的純邏輯 API（M6 的 GUI 只呼叫這層）。
- `crates/config::store`：`points/`、`keys/` 兩個資料夾的列出、讀取、存檔、另存、改名、刪除，開啟其他位置的檔案；`settings.json`（含 `HandSettings`）的讀寫。
- 原子存檔：純邏輯部分在 `config`，Windows 的檔案替換在 `win-platform::fs`。
- `docs/compat.md`：新舊行為對照表。

### 不做
- GUI（M6）。手牌的校準互動（M6）；這裡只提供寫入校準結果的 API。

## 匯入器

從點位 JSON 匯入點位檔時，讓呼叫端指定 `mode`（預設 `six`）。從功能 JSON 匯入預設按鍵檔不需要模式。舊檔沒有手牌資訊，不動 `settings.json` 的手牌設定。轉出的新檔預設存在原檔旁邊（檔名加「（新版）」），呼叫端可以指定別的位置；原檔不動。

| 來源 | 辨識方式 | 轉換重點 |
|---|---|---|
| AHK 點位 JSON → 點位檔 | `schema: "baauto-points-v1"` 或有 `bindings` | `function` 為 `normal`／`normal_point` → 新格式的普通點位；`alternating`（拖曳）→ 點位檔裡一筆功能為牌鍵的項目（名稱、熱鍵保留），拖曳終點座標捨棄，報告列出每一個被轉換的點名稱與座標；`hotkey` 缺少時讀 `key`；`points` 可能是物件或缺欄位 |
| 更舊的 `keys` 格式 → 點位檔 | 有 `keys` 陣列 | `rel_x/rel_y` 直接用；只有 `x/y` 時，需要 `width/height` 或 `screenW/screenH` 當基準 |
| 功能按鍵 JSON → 預設按鍵檔 | `schema: "baauto-functions-v1"` | `hotkeys` 對應 23 個 role，寫進 `presets`；`timing_ms` 只當「來源值」 |
| MuMu 按鍵設定 | 有 `keymaps` | 轉成**候選點**，之後在介面上決定要不要加成普通點位 |

- 兩種舊檔**分開**轉：功能 JSON 變成一個預設按鍵檔，點位 JSON 變成一個點位檔，不合併。
- 舊座標直接沿用（見下方「座標」），沒有「舊座標」狀態。

**AHK 版留下的資料問題，匯入時都要處理**

- 檔名可能是 `xxx.json.json`（`SubStr(path, -4)` 的 bug），照樣接受。
- 數值欄位可能是 `false`／`true`（`JsonDump` 把 0、1 寫成布林）。座標等數值欄位遇到布林，`false` → 0、`true` → 1，並在報告裡記一筆。
- 綁定 ID 可能重複或和新產生的 ID 撞號；`point_id` 可能指向不存在的點；點可能沒有任何綁定引用（AHK 刪綁定時不刪點）。合併成新格式時重新編號、刪掉孤兒，報告附舊 ID → 新 ID 的對照。
- `timing_ms` 寫了但 AHK 從來沒讀，所以真正生效的一直是程式裡寫死的值。匯入成 `timing.source = "imported_legacy"`、`verified: false`，報告註明 AHK 實際的等待會被 Windows 計時粒度拉長（例如 16ms 實際約 31ms）。
- 預設按鍵設成左鍵：匯入後停用該功能，報告說明原因。

**座標**

- 點位 JSON 與 `keys` 格式的 0–1 座標是「相對整個鎖定螢幕」。遊戲一律全螢幕 16:9，這就是新版的遊戲畫面座標，直接寫進點位檔，不換算。
- 只有絕對 `x/y`、又沒有基準尺寸時，**不要猜**：標記成「需要解析度」，等 GUI 問使用者當時的解析度，再換成 0–1。

**MuMu**

照 AHK 版 `ImportMuMuCandidates` 的規則：
- 略過 `key.device == "mouse"`、`text` 空白的項目。
- 一般項目用 `rel_work_position`，沒有就用 `icon.rel_position`。
- `type == "Macro"`：從 `press_actions` 找 `click_rel:(x,y)` 或 `curve_rel:(x,y)`，找不到再看 `release_actions`；和基準位置相同就略過。
- 座標不在 0–1 或落在三個忽略區（技能卡區、暫停鍵等，換算自 2560×1440）就略過。
- 產生候選點，名稱用觸發鍵文字。

## AHK 熱鍵字串解析

- 修飾符號 `^`（Ctrl）、`!`（Alt）、`+`（Shift）、`#`（Win），以及左右側前綴 `<`、`>`。
- 前綴 `$`、`*`、`~`：`*` 對應 `ModsMatch::AtLeast`。AHK 版所有熱鍵都用 `$*` 註冊，所以匯入預設一律 `AtLeast`。`~`（不攔截原鍵）在新版不支援，遇到就報告。
- 鍵名：字母、數字、F1–F24、Space、Tab、Esc／Escape、Enter、方向鍵、LButton、RButton、MButton、XButton1、XButton2、滾輪；大小寫不分。
- 無法解析的字串不丟例外：該項停用並寫進報告。

## 編輯 API（`config::editor`）

AHK 版的介面 bug 都來自「用名稱＋熱鍵反查資料」和「選取狀態沒清」。新版所有操作都用 ID：

```rust
impl KeysEditor {            // 預設按鍵 JSON
    pub fn set_preset(&mut self, role: Role, hotkey: Option<Chord>, enabled: bool, exact: bool) -> Result<(), EditError>;
    pub fn reset_preset(&mut self, role: Role);
    pub fn set_timing(&mut self, patch: TimingPatch) -> Result<(), EditError>; // 任何修改都把 verified 改回 false
}
impl HandEditor {            // settings.json 的手牌設定，改了馬上存
    pub fn set_slot_keys(&mut self, keys: [Key; 5]) -> Result<(), EditError>;
    pub fn set_card_auto_repeat(&mut self, policy: AutoRepeatPolicy);
    pub fn set_card_ui_side(&mut self, side: CardUiSide);
    pub fn set_slot_order(&mut self, order: SlotOrder);
    pub fn set_hand_rect(&mut self, mode: Mode, side: CardUiSide, rect: HandRect); // 寫入校準結果
}
impl PointSetEditor {        // 點位 JSON
    pub fn set_mode(&mut self, mode: Mode);                                // 六人／十人
    pub fn add_point_from_candidate(&mut self, c: CandidateId) -> PointId;   // 一律建立新項目（普通點位）
    pub fn add_card_item(&mut self, name: String) -> PointId;               // 新增一筆牌鍵
    pub fn update_item(&mut self, id: PointId, patch: ItemPatch) -> Result<(), EditError>; // 名稱、熱鍵、功能、座標
    pub fn delete_item(&mut self, id: PointId);
    pub fn move_to_candidates(&mut self, id: PointId) -> CandidateId;       // 「移回候選」
}
pub fn analyze(points: &PointSet, keys: &KeysProfile) -> ConflictReport; // 衝突的項目排在最前面，附原因
```

- `set_mode`：只改點位檔的 `mode`；格數和手牌矩形跟著從 `HandSettings` 取，不改預設按鍵檔。
- 每個修改都回傳是否影響衝突狀態，給 GUI 即時更新。

## 檔案管理（`config::store`）

```text
<程式資料夾>/
  settings.json           這台電腦的設定：手牌（位置、順序、四組矩形、每格按鍵、自動重複）、EngineParams、目前的點位檔與預設按鍵檔、兩列各自開著的分頁（含其他位置的檔案）、介面偏好
  points/                 點位 JSON（含六人／十人）
    範例.json
    <每個關卡或 Boss 一份>.json
    .trash/
  keys/                   預設按鍵 JSON
    預設按鍵.json
    <使用者自建>.json
    .trash/
```

- 第一次啟動沒有資料夾時，從 M1 的範本建立 `points/範例.json`、`keys/預設按鍵.json`，`settings.json` 的手牌設定用 `HandSettings::default()`。
- 兩種檔都提供：列出、讀取、存檔、另存新檔（任何資料夾）、改名、刪除（移到該資料夾的 `.trash/`）、開啟任何位置的檔案（路徑記在 `settings.json`，不複製）、關閉分頁（只從開著的清單拿掉，不動檔案）。開著的清單至少留一個。
- 檔名就是名稱；不能有 Windows 不允許的字元，重名時拒絕。
- 開檔一律先看內容判斷種類（點位、預設按鍵、舊版 AHK 點位、舊版 AHK 功能、更舊的 `keys` 格式），不看資料夾或檔名。開到舊格式的檔：走匯入器，轉出的新檔預設存在原檔旁邊，原檔不動。
- 路徑相容規則照規格 §10「任何路徑的 JSON」：UNC、OneDrive、中日文、空白、長路徑、UTF-8／UTF-8 BOM／UTF-16 BOM、同一個檔不重複開、外部修改偵測、檔案不見時標「找不到檔案」。
- `settings.json` 讀不到或壞掉：用預設值啟動，原檔改名保留，報告給使用者。記住的檔案不見了：退回範本，報告給使用者。

## 存檔

- 點位檔、預設按鍵檔與 `settings.json` 都用同一套：寫到同一個資料夾的暫存檔 → 重新解析一次確認內容正確 → flush（Windows 上 `FlushFileBuffers`）→ 用 `ReplaceFileW` 換掉舊檔並留 `.bak`；目標檔不存在時用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`。
- 任何一步失敗：舊檔保持原樣，回傳錯誤。
- 副檔名用 `Path` 處理：已經是 `.json` 就不再加。
- 輸出的 JSON 要好讀：固定欄位順序、兩格縮排、時間用毫秒、熱鍵用可讀字串，讓使用者可以直接手改。
- 讀到未知的新 `schema` 版本就拒絕，不猜。

## 新舊行為對照（寫進 `docs/compat.md`）

至少列出這些，每一項寫「AHK 版」「新版」「為什麼」：

1. 拖曳點位拿掉了，改成牌鍵：按牌鍵熱鍵時，游標在第幾張牌上就送那張牌的數字鍵。舊的拖曳終點座標不再使用。
2. 一樣是兩種檔：點位 JSON（現在多了六人／十人，拖曳點位變成牌鍵項目）和預設按鍵 JSON（就是原本的功能 JSON：預設按鍵＋時間）。各自切換、各自存檔。
3. 手牌左右位置、範圍、每格按鍵是這台電腦的設定（`settings.json`），預設右邊，要和遊戲選項「スキルカードUI位置」一致；校準一次，所有點位檔共用。
4. Esc 變成獨占資源：卡幀開牌、雙 Esc、普通暫停同時觸發時，後到的會被拒絕。
5. 卡幀開牌在停止或失焦時不補結尾的 Esc，改提示人工確認暫停狀態。
6. 按住熱鍵時的自動重複：新版可以選。匯入舊設定時一般功能預設 `FollowTypematic`，和 AHK 版一樣；牌鍵預設不重複。
7. 時間參數：舊值只當起點，標成未驗證；實際值要經過 M5 校準。
8. 座標：和 AHK 版一樣相對整個螢幕（遊戲一律全螢幕 16:9），舊座標直接沿用；換 16:9 的其他解析度也不用重錄。不支援視窗化。
9. 左鍵不能當觸發鍵。
10. 主視窗按 Esc 不再關閉程式。
11. 普通點位衝突的請求會被拒絕並顯示原因（和 AHK 版一樣不延後執行）。

## 合成測試檔

在 `tests/fixtures/synthetic/` 建立，涵蓋：`.json.json` 檔名、數值是布林、`points` 是物件、缺 `id`、重複 `id`、`point_id` 指向不存在的點、孤兒點、空熱鍵、預設功能互相衝突、預設功能設成左鍵、多個拖曳點位（`alternating`）、點位 JSON 和功能 JSON 成對、只有其中一個、`keys` 格式有／沒有基準尺寸、MuMu 的滑鼠項目、忽略區、`Macro` 只有 `curve_rel`。

## 測試與驗收

- [ ] 每個合成測試檔都有一個測試，比對匯入後的 `KeysProfile`／`PointSet` 與報告（期望值存成 JSON）。
- [ ] `import_alternating_becomes_card_items`：每個拖曳點位都變成一筆功能為牌鍵的項目（名稱、熱鍵保留），報告列出被捨棄的終點座標。
- [ ] `import_functions_json_to_keys`、`import_points_json_to_pointset_with_mode`：兩種舊檔各自轉成對應的新檔，不合併；點位檔帶上呼叫端指定的模式。
- [ ] `tests/fixtures/legacy/` 裡每個真實檔案都能匯入，沒有 panic；報告列出所有修正。
- [ ] 熱鍵解析：AHK 版 23 個預設熱鍵全部正確；加上 `^+a`、`<^F9`、`*XButton1`、`~a`（報告不支援）。
- [ ] 舊座標直接沿用：0–1 座標匯入前後完全相同；`keys` 格式只有絕對座標又沒有基準尺寸時標記「需要解析度」，給了解析度之後換算正確。
- [ ] 編輯 API：同名同熱鍵的兩個點位可以各自選取、各自刪除；從候選點新增不會覆蓋任何既有項目。
- [ ] `set_mode_six_to_ten_and_back`：格數、`slot_keys`、手牌矩形、4–5 號功能的變化和回報都正確。
- [ ] 檔案管理：第一次啟動建立範本、另存、改名、重名拒絕、刪除移到 `.trash/`、開啟其他位置的檔案、`settings.json` 壞掉或記住的檔案不見時的復原；兩種檔都要測。
- [ ] 存檔往返：存 → 讀 → 完全相同；模擬寫到一半失敗，舊檔不變；輸出的 JSON 欄位順序固定。
- [ ] 路徑：`paths_unicode_spaces_unc_long`（中日文、空白、UNC 字串、超過 260 字元都能讀寫；Linux 上測得到的部分先測，UNC 與 Windows 長路徑放 `#[cfg(windows)]`）、`open_detects_kind_by_content`（點位檔放在 keys 資料夾、預設按鍵檔改名成 points.json 都判斷正確）、`same_file_not_opened_twice`（大小寫、斜線不同的同一個檔）、`encoding_utf8_bom_utf16`、`external_change_detected_before_save`、`missing_file_tab_marked`、`legacy_converted_next_to_original`。
- [ ] `docs/compat.md` 完成。

## 複查清單（抽查）

- 布林轉數值是否只套用在數值欄位，不會把真正的布林設定改掉？
- ID 重新編號後，所有引用都跟著改了嗎？拖曳點位轉成牌鍵項目時，有沒有漏掉或重複？
- 只有絕對座標又沒有基準尺寸時，是否真的沒有偷偷用某個預設解析度？
- 存檔失敗的每一條路徑，舊檔是否都還在？

## 貼給 Codex 的開場訊息

```text
請完成里程碑 M4。先讀 AGENTS.md、docs/spec/BAauto-rs-融合架構-v0.4.md（重點 §9A、§10、§11）、docs/tasks/M4-compat.md，以及 reference/BAauto.ahk 裡的 ImportPoints、ImportMuMuCandidates、SaveFunctions、JsonDump、RegisterHotkeys。M1 已完成，兩種格式（點位檔 baauto-rs-points-v1、預設按鍵檔 baauto-rs-keys-v1）和 HandSettings 在 M1 定義。

這一步全部是純邏輯：舊格式匯入器（功能 JSON → 預設按鍵檔、點位 JSON → 點位檔，分開轉；拖曳點位轉成牌鍵項目）、AHK 熱鍵解析、點位資料整合、座標換算、config::editor、config::store（points/、keys/ 與 settings.json）、存檔流程，加上 docs/compat.md。Windows 的檔案替換 API 放在 win-platform::fs，Linux 上用 rename 走同一套測試。

做法：先建立 tests/fixtures/synthetic/ 的合成檔和期望輸出，再實作到全部通過。如果 tests/fixtures/legacy/ 是空的，在回報裡提醒我補真實檔案。完成後照 AGENTS.md 的格式回報。
```
