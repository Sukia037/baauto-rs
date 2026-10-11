# 實際 GUI 截圖

以下 24 張 PNG 由原生 macOS eframe／OpenGL framebuffer 截圖事件取得，使用離線預覽與範本資料。不是 HTML Demo 或合成介面圖。四個分頁各有淺色／深色、100／150／200% **應用程式 zoom**。

| 分頁 | 淺色 100% | 深色 100% | 淺色 150% | 深色 150% | 淺色 200% | 深色 200% |
|---|---|---|---|---|---|---|
| 點位 | [圖](light-points-100pct.png) | [圖](dark-points-100pct.png) | [圖](light-points-150pct.png) | [圖](dark-points-150pct.png) | [圖](light-points-200pct.png) | [圖](dark-points-200pct.png) |
| 預設按鍵 | [圖](light-keys-100pct.png) | [圖](dark-keys-100pct.png) | [圖](light-keys-150pct.png) | [圖](dark-keys-150pct.png) | [圖](light-keys-200pct.png) | [圖](dark-keys-200pct.png) |
| 時間 | [圖](light-timing-100pct.png) | [圖](dark-timing-100pct.png) | [圖](light-timing-150pct.png) | [圖](dark-timing-150pct.png) | [圖](light-timing-200pct.png) | [圖](dark-timing-200pct.png) |
| 紀錄 | [圖](light-logs-100pct.png) | [圖](dark-logs-100pct.png) | [圖](light-logs-150pct.png) | [圖](dark-logs-150pct.png) | [圖](light-logs-200pct.png) | [圖](dark-logs-200pct.png) |

截圖可檢查中文字型、固定三欄、選取、鍵帽、完整路徑內容與捲動、淺深色。較大 zoom 的原生視窗會受桌面可用範圍限制；Retina framebuffer 的 PNG 像素也不是 egui 邏輯尺寸。各檔實際像素、zoom、SHA-256 記於 [manifest.json](manifest.json)。

這些圖片**不能代替 Windows OS DPI 驗收**。Windows 系統字型、100／150／200% 及混合 DPI、透明校準／錄點覆蓋層與真正遊戲目前均未執行驗收。請依 [M6 清單](../acceptance-m6.md) 使用 `tests/windows/M6.ps1 -Screenshots` 在每種真實 OS DPI 下重跑，將 `windows-current-dpi` 輸出分別保存並記錄螢幕設定。

自行重現離線截圖：建置 app 後執行 `baauto-app --preview --data-dir <測試資料夾> --qa-dir <輸出資料夾> --qa-tab 0 --qa-scale 1`；分頁 0／1／2／3 分別為點位／按鍵／時間／紀錄，深色加 `--qa-dark`，zoom 可用 1／1.5／2。程式等載入提示消失後截圖並正常退出；QA 不繼承前一次保存的視窗大小。
