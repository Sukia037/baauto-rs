# M1–M6 提交與驗證總表

GitHub：`Sukia037/baauto-rs`（private）。M1–M6 的來源實作、可攜式測試及 Windows GNU 交叉檢查已完成，每個里程碑保留至少三筆實質提交。**原生 Windows、真實舊檔、遊戲、性能及長時間驗收尚未完成**；各報告列出限制，不以編譯通過代替實測。

| 里程碑 | 測試／契約 | 功能實作 | 實質修正 | 回報 |
|---|---|---|---|---|
| M1 core/config | `c20986b` | `860405f` | `680d544` | [M1](M1-report.md) |
| M2 Hook/Engine/Driver | `6df57fc` | `7c697cb` | `29523d8` | [M2](M2-report.md) |
| M3 IPC/監督/健康 | `43a4900` | `cd8d2c0` | `d744eb9` | [M3](M3-report.md) |
| M4 遷移/編輯/檔案 | `74f493f` | `5d4431b` | `37e03c7` | [M4](M4-report.md) |
| M5 量測/校準/實驗 | `f6d2473` | `b925812` | `e6cda57` | [M5](M5-report.md) |
| M6 GUI/擷取/校準 | `4e2b039` | `dc88c21` | `c2fee65` | [M6](M6-report.md) |

M6 另保留 `bfead8a` 修正截圖模式繼承歷史視窗尺寸、`4e3fe50` 補齊選取項目的功能說明與時間用途，以及一筆 `docs(M6)` 交付文件與 24 張實際 GUI 截圖。初始設計 `06520aa`、提交規則 `faf280c` 不計入里程碑數量；未 squash 上述提交。

## 本機驗證

環境：macOS、Rust stable 1.99.0、edition 2024。workspace 154 個測試全部通過，fmt、Clippy `-D warnings` 通過；Windows GNU 一般版與 all-features／all-targets Clippy 交叉檢查通過，含 test-hooks 與實驗功能的 Windows 分支。Linux 與 Windows CI 已設置；此表只報告實際本機結果，不宣稱遠端 CI 或 Windows 執行結果。

M6 四分頁 × 淺深色 × 三種應用程式 zoom 的 24 張原生 macOS GUI 截圖及雜湊見[截圖索引](../screenshots/README.md)。這不是 Windows OS DPI 驗收。

## 原生驗收入口

| 項目 | 操作入口 | 仍缺的證據 |
|---|---|---|
| Hook、SendInput、零／短送、幾何、共享帳本 | `tests/windows/M2.ps1` | 原生接收器、停止及配對放開 |
| 監督、崩潰／失聯、退出後釋放 | `tests/windows/M3.ps1` | 原生故障注入與 Raw Input 健康 |
| 原子存檔、匯入、Unicode／長路徑 | `tests/windows/M4.ps1`、真實舊檔 | Windows 檔案系統與使用者樣本 |
| DXGI／PresentMon／校準／八組 A/B | [bench-howto](../bench-howto.md)、`tests/windows/M5.ps1` | 原生性能／六分類／足量樣本；不產生虛構 verified |
| GUI、真實 DPI、擷取、23 功能、長測 | [M6 驗收清單](../acceptance-m6.md)、`tests/windows/M6.ps1` | 8 小時接收器、六／十人各 1 小時非排名遊戲 |

目前正式預設維持 Hook TIME_CRITICAL、Engine NORMAL、spin=0。設定範本的時間是未驗證值；原生校準與性能結果沒有完成前，不能聲稱達到遊戲內成功率、延遲分位數或零卡鍵。

待決定事項集中在 [decisions.md](../decisions.md)，包括全域停止／錄點的鍵盤限制、GUI 操作逾時、擷取／取消關閉後保持停止，以及先前里程碑的相容性語意。
