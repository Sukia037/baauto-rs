# 在 Windows 上試跑（不用遊戲）

沒有遊戲也能測大部分功能：`baauto-receiver.exe` 是一個全螢幕的「假遊戲」視窗，會記錄巨集送出的每個按鍵和點擊。把 app 的目標程式設成它就行。

## 1. 安裝工具（只要一次）

1. 安裝 Rust：PowerShell 執行 `winget install Rustlang.Rustup`，或到 https://rustup.rs 下載 `rustup-init.exe`。
2. 安裝過程如果問要不要裝 Visual Studio，選自動安裝（選項 1）。已經有 Visual Studio 的話，確認有勾「使用 C++ 的桌面開發」。
3. 關掉 PowerShell 再開一個新的，輸入 `rustc --version`，有版本號就好了。

## 2. 建置

在專案資料夾（有 `Cargo.toml` 的那一層）開 PowerShell：

```powershell
cargo build --release --workspace
```

第一次會下載套件，大約要幾分鐘。完成後執行檔在 `target\release\`。

想順便跑完整的自動檢查（格式、clippy、測試、正式版建置）：

```powershell
powershell -ExecutionPolicy Bypass -File tests/windows/M6.ps1
```

## 3. 開「假遊戲」和 app

開兩個 PowerShell 視窗：

```powershell
# 視窗 1：假遊戲（全螢幕，按 Alt+F4 關閉並存檔）
.\target\release\baauto-receiver.exe --hand six --side right --output receiver.csv
```

```powershell
# 視窗 2：BAauto，把假遊戲當成遊戲
.\target\release\baauto-app.exe --target-exe baauto-receiver.exe --data-dir C:\BAauto-test
```

`--data-dir` 是測試用資料夾，裡面會放範本 JSON、settings.json 和 logs，不會動到別的地方。

如果首頁頂部出現紅色警告「緊急停止鍵 Ctrl+F9 被其他程式佔用」，「啟用」會暫時不能按。點警告裡的「前往「預設按鍵」改鍵」，把「緊急停止」改成另一組沒被佔用的鍵盤組合並儲存，警告消失後就能啟用，不用重開。錄點鍵被佔用只會警告，不影響啟用。

## 4. 要看的地方

2026-10-11逐項接收器補測發現：預設慢卡幀（Up）只送第一組Esc；原始`double_esc_second`晚到容許值0ms，晚到即取消第二組。2026-10-12已整組移除晚到上限（見`docs/decisions.md`「移除晚到上限」），第二組Esc一定送出；重跑接收器確認前不要把這項當已通過；[補測結果與原始紀錄](test-results/windows-20261011/expanded/README.md)也列出已測的其餘動作、各格牌鍵和錄點。

**介面**
- [ ] 中文字沒有方框，四個分頁、淺色／深色都正常。
- [ ] 視窗拉到最小，文字不會疊在一起，清單可以捲到底。

**點位**
- [ ] 按「＋ 錄製」，等狀態列出現「Hook 已就緒」，切到假遊戲點一下，候選點位出現，位置合理。
- [ ] 選候選，按「帶入點位按鍵清單 →」，接著按一個鍵（例如 c），清單出現新的一行。
- [ ] 點清單裡的鍵帽可以重新設定熱鍵；Esc 取消、Backspace 清除。
- [ ] 熱鍵和預設按鍵重複時，那一行變紅並顯示「和預設按鍵「…」重複，停用」。
- [ ] 切六人／十人、改名稱，底部出現「有未儲存的變更」，按儲存後消失。

**實際送鍵（看假遊戲有沒有收到）**
- [ ] 按右上「啟用」，切到假遊戲：按 w 收到 1；按住 2 一直重複「1＋左鍵」；按 Down 收到 Esc、Esc。
- [ ] 按住 z（普通點位）游標移到該點並按住左鍵，放開 z 就放開。
- [ ] 按住 q，游標移到手牌第 1、2、3 格：一直重複收到「該格數字＋在牌鍵點位的左鍵」，每次點完游標回到牌上；按住時換格會換數字；不在手牌上時暫停；放開 q 就停。
- [ ] 按住任何鍵時按 Ctrl+F9，所有鍵都放開，狀態變「已停止」，不會自己恢復。

**手牌**
- [ ] 選「出牌」，右側看得到手牌設定；按「在遊戲上校準」，框出現在假遊戲上，可拖角和邊，Esc 取消。
- [ ] 右邊／左邊、順序、每格按鍵改了馬上生效。

**檔案**
- [ ] 「開啟…」可以開其他磁碟、中文路徑、OneDrive 裡的 JSON；把 JSON 拖進視窗也可以。
- [ ] 分頁的 × 可以「關閉分頁」或「刪除檔案」（移到同資料夾的 `.trash`）；只剩一個分頁時不能關。

## 5. 回報

哪一步怪怪的，截圖加上一句「做了什麼、看到什麼」就好。需要更多資料時，再把 `C:\BAauto-test\logs` 和 `receiver.csv` 給我。
