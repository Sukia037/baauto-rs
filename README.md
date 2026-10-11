# BAauto-rs

Blue Archive PC 版輸入輔助工具的 Rust 重寫版，取代原本的 AutoHotkey 腳本 `BAauto.ahk`。M1–M6 的實作、可攜式測試與 Windows 原生短程補測已完成；目前仍有失敗項目，尚未通過完整驗收。

## 目前驗證狀態（2026-10-11）

- Windows `fmt`、`clippy`、160項自動測試與正式release建置通過，3項測試略過。
- 接收器逐項對照23項預設功能：使用替代全域熱鍵時22項通過，**原始時序的慢卡幀失敗，只送第一組Esc**。原因是第二組Esc的排定時間等於最晚期限；2026-10-12已整組移除晚到上限，第二組Esc一定送出（macOS 156項測試與Windows交叉clippy通過），**尚待Windows重跑接收器確認**。
- 六人／十人各格牌鍵、左右與反向順序、框外不送鍵、錄點、右鍵／側鍵／Space／Tab擷取通過。這些是測試版本的Windows輸入注入，尚未驗收實體硬體與真實遊戲。
- 本機Ctrl+F9被其他程式佔用。2026-10-12起熱鍵被佔用時app不會卡住：首頁頂部顯示紅色警告並停用「啟用」，到「預設按鍵」改鍵儲存後立即恢復，不用重開（尚待Windows實機確認）。故障注入5項通過、掛起案例1項仍失敗。
- 校準覆蓋層顯示、前景切換與逾時已測；拖拉／確認儲存、完整GUI切檔／匯入、Windows DPI仍待驗收。超過20分鐘的長時間與遊戲測試依使用者決定不做。

完整CSV、測試profile、修正、重跑指令與未測項目見[Windows補測報告](docs/test-results/windows-20261011/expanded/README.md)。正式release已重新建置，不含`test-hooks`／`fault-injection`。

## 從哪裡開始看

| 想知道 | 看這裡 |
|---|---|
| 整體設計 | `docs/spec/BAauto-rs-融合架構-v0.4.md` |
| 要做哪些事、照什麼順序 | `docs/tasks/README.md` |
| 給 Codex 的規則 | `AGENTS.md` |
| Windows安裝與假遊戲試跑 | [Windows試跑清單](docs/windows-試跑.md) |
| 最新實測與已知失敗 | [Windows補測報告](docs/test-results/windows-20261011/expanded/README.md) |
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

不用遊戲也能試：在兩個PowerShell視窗依序啟動接收器與app，再切到接收器測試輸入。

```powershell
# 第一個視窗：假遊戲接收器，正常關閉後保留CSV與同名JSON。
.\target\release\baauto-receiver.exe --hand six --side right --output receiver.csv
# 第二個視窗：控制介面。
.\target\release\baauto-app.exe --target-exe baauto-receiver.exe --data-dir C:\BAauto-test
```

若首頁頂部出現紅色的熱鍵佔用警告，點「前往「預設按鍵」改鍵」換一組沒被佔用的鍵盤組合並儲存即可；逐項操作見[Windows試跑清單](docs/windows-試跑.md)。

macOS／Linux 可查看離線介面：

```sh
cargo build -p baauto-app
./target/debug/baauto-app --preview --data-dir /tmp/baauto-preview
```

離線預覽不會送出遊戲輸入。Windows 控制程序仍可使用 `--headless`；量測與校準工具見 [bench-howto](docs/bench-howto.md)。

## 實作與驗收

- [M1–M6 提交與驗證總表](docs/implementation/status.md)：每個里程碑至少三個實質提交。
- [實際介面截圖](docs/screenshots/README.md)：四分頁＋牌鍵詳細、淺深色、三種應用程式縮放，共30張；來源是Linux離線預覽，不能代替Windows DPI／遊戲驗收。
- [Windows原生截圖](docs/screenshots/windows-20261011-current-dpi/)：10張淺／深色介面，加上校準覆蓋層與錄點候選；目前縮放的截圖不代表100%／150%／200% OS DPI均通過。
- [最新Windows實測](docs/test-results/windows-20261011/expanded/README.md)：23項接收器對照、各格牌鍵、錄點與擷取；列出失敗與未驗收項目。
- [M6 Windows 驗收清單](docs/acceptance-m6.md)：錄點、側鍵、停止、校準、切檔與匯入；長時間測試不做。
- [M6 實作回報](docs/implementation/M6-report.md)：介面、安全界線、依賴、待決定事項及複查位置。

手上的舊點位 JSON、`function_keys.json`、MuMu 設定可用「匯入舊檔／MuMu…」轉換；目前只有合成相容性測試，仍需真實舊檔核對。時序範本保持未驗證；只有 M5 完整證據通過後才能載入已驗證的校準結果。
