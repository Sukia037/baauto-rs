# 內嵌繁體中文字型

`NotoSansTC-Regular-subset.ttf` 是 Noto Sans TC 的靜態 400 字重子集，作為系統 Microsoft JhengHei UI 不存在時的後備字型。Windows 啟動時先讀系統 TTC，按 name table 找 UI family；macOS 離線預覽使用此後備字型。

上游：Google Fonts `ofl/notosanstc/NotoSansTC[wght].ttf`，版本 `2.004-H2`。原始可變字型 11,941,968 bytes，SHA-256 `864727d210d54f2537bbe23b3a839436c3992af72de9322af5270897246bd44f`。授權文字保留在 `OFL-NotoSansTC.txt`。

以獨立工具環境的 fonttools 4.66.1 將 wght 固定為 400，再取 Latin、CJK／假名、全形及標點範圍；產物 18,229 個 Unicode codepoint，6,235,988 bytes，SHA-256 `b774d1cb4350a2488713021ead4cd72689d919e7e7fb491239932316f9fb1586`。字型只在 GUI 啟動時載入，不涉及 Hook／Engine 熱路徑。較廣的 CJK 範圍用於使用者中日文檔名與路徑；不是保證所有 Unicode 字元皆可顯示。
