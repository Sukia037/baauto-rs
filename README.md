# BAauto-rs

Blue Archive PC 版輸入輔助工具的 Rust 重寫版，取代原本的 AutoHotkey 腳本 `BAauto.ahk`。M1–M6 的實作、可攜式測試與 Windows 交叉編譯已完成；原生 Windows／遊戲與長時間驗收仍待執行。

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

## 建置與啟動

Windows 安裝目前 Rust stable 與 Visual Studio C++ Build Tools 後，在 repo 執行：

```powershell
cargo build --release --locked --workspace
.\target\release\baauto-app.exe
```

引擎 `baauto-engine.exe` 要和 app 放在同一資料夾。首次啟動會建立兩種範本 JSON 和 `settings.json`，資料預設存在執行檔旁；可用 `--data-dir <資料夾>` 指定其他位置。每個點位／預設按鍵 JSON 仍可單獨放在任何路徑，透過開啟對話框或拖入加入分頁。介面預設保持停止，按「啟用」才開始。

macOS／Linux 可查看離線介面：

```sh
cargo build -p baauto-app
./target/debug/baauto-app --preview --data-dir /tmp/baauto-preview
```

離線預覽不會送出遊戲輸入。Windows 控制程序仍可使用 `--headless`；量測與校準工具見 [bench-howto](docs/bench-howto.md)。

## 實作與驗收

- [M1–M6 提交與驗證總表](docs/implementation/status.md)：每個里程碑至少三個實質提交。
- [實際介面截圖](docs/screenshots/README.md)：四分頁、淺深色、三種應用程式縮放，共24張；來源是 macOS 離線預覽，不能代替 Windows DPI／遊戲驗收。
- [M6 Windows 驗收清單](docs/acceptance-m6.md)：錄點、側鍵、停止、8小時接收器與六人／十人各1小時遊戲測試。
- [M6 實作回報](docs/implementation/M6-report.md)：介面、安全界線、依賴、待決定事項及複查位置。

手上的舊點位 JSON、`function_keys.json`、MuMu 設定可用「匯入舊檔／MuMu…」轉換；目前只有合成相容性測試，仍需真實舊檔核對。時序範本保持未驗證；只有 M5 完整證據通過後才能載入已驗證的校準結果。
