# AHK 與 BAauto-rs 功能對照

依據 reference/BAauto.ahk 與架構 v0.4；不是舊 archive 規格。

| 項目 | AHK 版 | 新版 | 原因 |
|---|---|---|---|
| 拖曳點位 | alternating 拖曳 | 每筆轉成牌鍵；游標所在手牌格決定輸出鍵，終點捨棄並報告 | v0.4 移除拖曳 |
| 兩種 JSON | 點位、功能檔 | 點位含 mode/items；按鍵含 23 presets/timing，各自存切 | 保留既有使用習慣 |
| 手牌 | 腳本／固定位置 | settings.json 四組矩形、左右位置、順序與五個鍵，全檔共用 | 配合遊戲「スキルカードUI位置」 |
| Esc | 多流程可能交錯 | 獨占資源，後到拒絕 | 動作順序可預測 |
| 卡幀停止 | 可補 Esc | 停止／失焦不補結尾 Esc，需人工確認暫停狀態 | 不於取消後產生新動作 |
| 自動重複 | 熱鍵 typematic | 一般功能匯入 FollowTypematic；牌鍵預設 Ignore，可改 | 避免意外多出牌 |
| 時序 | 程式寫死，timing_ms 存而未讀 | 來源值留報告，TimingProfile 標 imported_legacy、verified=false | 真值需要 M5 校準 |
| 座標 | 相對鎖定螢幕 | 相同 0–1，全螢幕 16:9；絕對座標無基準時待補解析度 | 不猜、不做視窗補償 |
| 左鍵觸發 | 可能和輸出衝突 | 禁止；匯入時停用並報告 | 可靠停止與持有帳本 |
| 主介面 Esc | 可關閉 | 不關閉（M6） | 避免誤退出 |
| 普通點位衝突 | 拒絕 | 拒絕並顯示原因，不延後 | 不執行已過時的請求 |
| ID 與候選 | 名稱＋熱鍵反查、孤兒／撞號 | 編輯只用 ID；匯入重編號；候選新增永遠是新項目 | 修復選取／覆蓋問題 |
| 修飾符 | AHK ^ ! + #，左右側前綴 | 解析、AtLeast；左右側暫轉通用修飾符並明確報告 | 現行 fixed modifier table 僅有四個通用 bit，待決定是否擴充 |
| ~ 與滾輪 | ~ 放行；滾輪是脈衝 | 識別後停用並報告，不假裝有配對 Down/Up | 現行 paired-key Hook 無這些語意，見 decisions |
| 任意位置 JSON | 路徑／副檔名與编码限制 | 依內容判斷；PathBuf；UTF-8/BOM/UTF-16；同檔分頁正規化 | 可直接手改、Unicode／長路徑 |
| 存檔 | 刪舊檔後寫 | 同目錄暫存、重新解析、sync/Flush、備份、原子替換 | 失敗保留可復原原檔 |

Windows 儲存使用 [ReplaceFileW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew) 與 [MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)。ReplaceFileW 的部分失敗可能把原檔改名，因此先保留備份，缺原路徑時先復原再回報；不把 write-through 說成所有網路磁碟或断電情況的保證。檔案正規化使用 [GetFinalPathNameByHandleW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfinalpathnamebyhandlew)。UNC、OneDrive 與特殊檔案系統需本機測試。
