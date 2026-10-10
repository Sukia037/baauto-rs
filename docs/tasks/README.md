# BAauto-rs 里程碑總覽

六個里程碑，一個里程碑開一個 Codex 任務。每個任務檔最後都有「貼給 Codex 的開場訊息」，整段複製貼上即可。

## 開始之前

1. 這個資料夾就是 repo 根目錄，在這裡 `git init` 就可以開始。裡面已經有：
   - `AGENTS.md`
   - `docs/spec/BAauto-rs-融合架構-v0.4.md`
   - `docs/tasks/`（這份總覽和六個任務檔）
   - `docs/decisions.md`（待決定事項，Codex 會往裡面加）
   - `reference/BAauto.ahk`：原本的 AHK 腳本，M4 對照行為用
2. M4 需要真實的舊設定檔。請把你手上的點位 JSON、`config/function_keys.json`、MuMu 按鍵設定檔各放幾份到 `tests/fixtures/legacy/`。

## 順序與依賴

```text
M1 core ──┬─> M2 win-input ─> M3 supervisor ─> M5 bench ─┐
          └─> M4 compat ───────────────────────────────────┴─> M6 app
```

M4 只依賴 M1，可以和 M2、M3 同時進行。

| 里程碑 | 內容 | 在哪跑 | 建議 effort | 完成後複查 |
|---|---|---|---|---|
| [M1 core](M1-core.md) | 動作狀態機（含牌鍵）、時間語意、點位 JSON 與預設按鍵 JSON 兩種格式（六人／十人、牌鍵、手牌設定）、範本、模擬器 | 雲端 Linux 可完成 | 高 | 建議：狀態機與時間語意 |
| [M2 win-input](M2-win-input.md) | Hook → 佇列 → Engine → Driver、共享記憶體、測試接收器 | 雲端寫、本機 Windows 測 | 高 | **必做** |
| [M3 supervisor](M3-supervisor.md) | 控制程序、IPC、停止協定、故障復原、故障注入 | 雲端寫、本機 Windows 測 | 高 | **必做** |
| [M4 compat](M4-compat.md) | 舊 AHK／MuMu 匯入（功能 JSON→預設按鍵檔、點位 JSON→點位檔、拖曳轉牌鍵）、兩種檔的管理、原子存檔、功能對照 | 雲端 Linux 可完成 | 中 | 抽查 |
| [M5 bench](M5-bench.md) | 時序統計、PresentMon、DXGI 判定、校準掃描、優先權實驗 | 統計在雲端，量測在本機 Windows | 中到高 | 建議：統計方法 |
| [M6 app](M6-app.md) | 三欄介面、點位／預設按鍵兩種 JSON 切換、手牌校準覆蓋層、錄點、報告入口 | 本機 Windows | 中 | 抽查 |

「雲端寫、本機 Windows 測」的意思：Codex 雲端任務通常是 Linux 容器，跑不了鍵盤鉤子和 SendInput，只能編譯檢查。這幾個里程碑要嘛用 Windows 上的 Codex CLI 做，要嘛雲端寫完後你在 Windows 上照任務檔的「手動驗收」跑一次。

## 每個里程碑的節奏

1. 開新任務，貼開場訊息。
2. 讓它先交測試清單和介面，再寫實作。
3. 看回報：驗收項目有沒有全部打勾、哪些測試沒跑、有沒有新增待決定事項。
4. M2、M3 做完後，把回報裡「建議複查」的檔案交給另一個模型（Astra 或 Opus）複查，用任務檔裡的複查清單。
5. 本機 Windows 驗收通過後，才開下一個里程碑。

## 複查時要給的東西

- 規格 v0.4、當次任務檔、`AGENTS.md`
- 該里程碑的 diff 或建議複查的檔案
- 任務檔最後的「複查清單」

請複查者只回報有具體失敗情境的問題：什麼輸入、什麼時序、會發生什麼錯。
