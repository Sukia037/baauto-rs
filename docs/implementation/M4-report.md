# M4 實作與驗收狀態

已完成 legacy 匯入、ID 編輯 API、兩種文件及完整 settings 的管理、同目錄原子存檔，以及 Windows 原生替換／路徑封裝。headless 與 standalone 接受完整 settings.json、UTF-8/BOM/UTF-16；headless 換檔先編譯成功才替換草稿，再走 M3 停止協定。首次 headless 啟動在程式資料夾建立範本。

## 驗收對應

12 個 compat 測試、11 個 store 測試，加原有 15 個 config 測試皆通過。五個合成来源各有測試與 JSON golden；完整比對點位、預設按鍵、MuMu 候選、待補解析度，以及匯入報告的修正類別／筆數。涵蓋兩張舊表合併、布林數值修正、重編 ID、孤兒與缺引用、拖曳轉牌鍵、AHK 23 個預設鍵、分開匯入、六人／十人切換、同名項目獨立編輯、ID 不重用、無效編輯保持原內容。

儲存測試涵蓋首次範本、讀寫往返、固定欄位順序、改名重名拒絕、.trash、最後分頁保護、內容辨識、同檔去重、損毀 settings 保留、遺失分頁標記、其他位置、編碼、Unicode／長路徑、外部修改、原檔旁轉換；分別注入半寫與替換失敗，舊檔保持原樣。更新 settings 失敗時，分頁操作還原記憶體，改名與 .trash 嘗試回復原路徑。

## 指令與未執行項目

macOS 通過 cargo fmt --check、cargo clippy --workspace --all-targets -- -D warnings、cargo test --workspace（121 個可攜測試），及 cargo clippy --workspace --all-targets --all-features --target x86_64-pc-windows-gnu -- -D warnings。

Windows 執行 tests/windows/M4.ps1；可傳 -UncRoot '\\server\share\test-folder' 跑 ignored UNC 測試。原生測試檢查 GetFinalPathNameByHandleW 去重、大小寫／斜線、Unicode／長路徑、備份、唯讀失敗。Windows 原生、UNC、OneDrive 未實跑。tests/fixtures/legacy 只有 README，没有真實舊 JSON；真實舊檔驗收未完成，合成測試不能代替。

## 差異與決策

左右側 AHK 修飾符暫正規化為通用修飾符，逐筆明確報告；~ 和滾輪辨識後停用，因現行配對事件協定沒有脈衝／放行語意，未假裝支援。完整支援需要後續決定。缺解析度保留原資料供重新匯入，不猜螢幕尺寸。Windows ReplaceFileW 部分失敗可能移走原路徑，先留同步備份並嘗試復原；磁碟故障連復原也失敗時只能保留備份並回報。CLI 仍接受 M1 hand-only 測試輸入，相容用途；正常 persistence 使用完整 settings。詳見 docs/decisions.md、docs/compat.md。

## 建議複查

crates/config/src/legacy.rs:150–399（來源分類、座標和修正報告）、editor.rs:18–160（ID 與交易式修改）、store.rs:266–312（暫存與替換）、store.rs:336–620（分頁與復原）、crates/win-platform/src/windows_impl/fs.rs:37–127（原生 identity／替換）、bins/baauto-app/src/main.rs:157（換檔先驗證再提交）。行數以此提交為準。

## 提交

74f493f：測試與合成資料；5d4431b：匯入／編輯／儲存實作；第三筆：失敗回復、原生驗收及 CLI 整合（見 git log）。
