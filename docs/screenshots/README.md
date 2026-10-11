# 實際 GUI 截圖

2026-10-11另有[Windows原生截圖](windows-20261011-current-dpi/)：10張目前zoom=1的淺／深色介面、[校準覆蓋層](windows-20261011-current-dpi/native-calibration-overlay.png)及[錄點候選](windows-20261011-current-dpi/native-recorded-candidate.png)。實測結果與未驗收項目見[Windows補測報告](../test-results/windows-20261011/expanded/README.md)；這組也不能當成100%／150%／200% OS DPI全部通過。

以下 30 張 PNG 是 app 本身畫出來的畫面（eframe 的 framebuffer 截圖），不是 HTML Demo。使用離線預覽與範本資料；版型照 [新介面 Demo](../pages/BAauto-新介面Demo.html) 的三欄版。五個畫面各有淺色／深色、100／150／200% **應用程式 zoom**。

| 畫面 | 淺色 100% | 深色 100% | 淺色 150% | 深色 150% | 淺色 200% | 深色 200% |
|---|---|---|---|---|---|---|
| 點位 | [圖](light-points-100pct.png) | [圖](dark-points-100pct.png) | [圖](light-points-150pct.png) | [圖](dark-points-150pct.png) | [圖](light-points-200pct.png) | [圖](dark-points-200pct.png) |
| 點位（選牌鍵） | [圖](light-points-card-100pct.png) | [圖](dark-points-card-100pct.png) | [圖](light-points-card-150pct.png) | [圖](dark-points-card-150pct.png) | [圖](light-points-card-200pct.png) | [圖](dark-points-card-200pct.png) |
| 預設按鍵 | [圖](light-keys-100pct.png) | [圖](dark-keys-100pct.png) | [圖](light-keys-150pct.png) | [圖](dark-keys-150pct.png) | [圖](light-keys-200pct.png) | [圖](dark-keys-200pct.png) |
| 時間 | [圖](light-timing-100pct.png) | [圖](dark-timing-100pct.png) | [圖](light-timing-150pct.png) | [圖](dark-timing-150pct.png) | [圖](light-timing-200pct.png) | [圖](dark-timing-200pct.png) |
| 紀錄 | [圖](light-logs-100pct.png) | [圖](dark-logs-100pct.png) | [圖](light-logs-150pct.png) | [圖](dark-logs-150pct.png) | [圖](light-logs-200pct.png) | [圖](dark-logs-200pct.png) |

這組圖在 Linux（Xvfb＋Mesa）上產生，字型是內嵌的 Noto Sans TC；Windows 上會改用微軟正黑體 UI，字寬略有不同。離線預覽沒有遊戲螢幕，所以像素座標是用截圖環境的螢幕（2560×1440）換算。各檔像素、zoom、SHA-256 記在 [manifest.json](manifest.json)。

這些圖片**不能代替 Windows OS DPI 驗收**。請依 [M6 清單](../acceptance-m6.md) 用 `tests/windows/M6.ps1 -Screenshots` 在每種真實 OS DPI 下重跑。

自行重現：`baauto-app --preview --data-dir <測試資料夾> --qa-dir <輸出資料夾> --qa-tab 0 --qa-scale 1`。分頁 0／1／2／3 是點位／預設按鍵／時間／紀錄，4 是點位分頁並選取牌鍵（顯示手牌設定）；深色加 `--qa-dark`，zoom 可用 1／1.5／2。程式等載入提示消失後截圖並正常退出；QA 不繼承前一次保存的視窗大小。
