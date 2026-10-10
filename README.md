# BAauto-rs

Blue Archive PC 版輸入輔助工具的 Rust 重寫版，取代原本的 AutoHotkey 腳本 `BAauto.ahk`。目前還在設計階段，程式碼由 Codex 依里程碑實作。

## 從哪裡開始看

| 想知道 | 看這裡 |
|---|---|
| 整體設計 | `docs/spec/BAauto-rs-融合架構-v0.4.md` |
| 要做哪些事、照什麼順序 | `docs/tasks/README.md` |
| 給 Codex 的規則 | `AGENTS.md` |
| 延遲和時間點怎麼跑 | `docs/pages/BAauto-rs-延遲時序.html`（用瀏覽器開） |
| 新介面長什麼樣 | `docs/pages/BAauto-新介面Demo.html`（三欄版，用瀏覽器開，可以點） |
| 還沒決定的事 | `docs/decisions.md` |
| 原本的 AHK 腳本 | `reference/BAauto.ahk` |
| 舊版文件 | `docs/archive/`（只是紀錄） |

## 這一版的功能重點

- 牌鍵：按熱鍵時，游標在第幾張牌上就送那張牌的數字鍵。
- 六人／十人（制約解除決戰）兩種模式，寫在點位 JSON 裡，跟著點位檔切換。
- 手牌 UI 左右位置都支援，預設右邊；手牌位置和範圍校準一次，所有點位檔共用。
- 兩種 JSON，像原本一樣分開：點位 JSON（每個關卡一份：六人／十人＋點位和牌鍵）和預設按鍵 JSON（23 個預設按鍵＋時間，像原本的 function_keys.json），各自切換，都可以直接手改。

## 你要先做的事

- 把手上的舊點位 JSON、`function_keys.json`、MuMu 按鍵設定檔放進 `tests/fixtures/legacy/`。
- 用 Codex 開 M1 任務：`docs/tasks/M1-core.md` 最後有開場訊息。
