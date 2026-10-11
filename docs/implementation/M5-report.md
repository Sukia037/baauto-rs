# M5 實作與驗收狀態

完成純解析／QPC對齊、nearest-rank分位數、Wilson／最少次數、非單調多段掃描與邊界細掃、SHA-256預承諾計畫、均勻相位、ROI/BMP／混淆矩陣、人工六分類 finalize、安全選值及原子寫回；新增 Calibration Arm／固定容量RunTrial佇列／結果與trial紀錄，單次時限到期可靠放開，停止不會重播。Windows有DXGI Desktop Duplication、PresentMon具名session、managed supervisor掃描、量測、八組A/B及CPU計時，所有遊戲輸出仍由Engine Driver。bench工作執行緒可以等待／寫檔，Hook與Engine穩態仍不配置。

## 測試與環境

M5新增bench9、core4、protocol1測試；涵蓋任務列的CSV缺欄／遺失／對齊、高分位門檻、Wilson已知值、非單調／FakeGame、計畫hash／驗收门檻、相位、ROI輸出、人工分類、一般session拒絕Trial、trial時間覆寫、到期釋放／停止不復活、1000次trial零配置、IPC session／cmdseq／輸出候選限制。

macOS通過fmt、workspace Clippy -D warnings、cargo test；WindowsGNU一般版與all-features Clippy交叉檢查通過。這些是可攜測試與型別檢查，不是Windows原生驗收。Windows執行 tests/windows/M5.ps1，手動DXGI ignored測試需真的接收器與實體W。操作與未完成清單在 docs/bench-howto.md。

未執行：DXGI顏色與接收器QPC核對、PresentMon原生对齊、八組A/B、滿載停止分布、每格接收器测試的原生覆核、真實非排名遊戲verified校準。沒有真實Windows／遊戲，不能產生可信verified或性能結論。scan維持未驗證直到完整人工分類及所有門檻通過。

## 差異與決策

雙側Wilson最少765次，較任務約600次嚴格；螢幕16:9依權威規格，沒有視窗化內容區；平均ROI差異只作候選偵測，完整六分類人工核對不能省略。工具bench crate内部core/win分模組而非新workspace子crate，純邏輯不依赖Windows。額外sha2用於穩定SHA-256。尚無A/B數據，預設優先權／自旋不變。部分停止來源尚缺原始請求戳，報表明示觀察時間回退，不把它当完整端到端上界。詳見docs/decisions.md。

## 建議複查

tools/bench/src/analysis.rs（CSV與統計）、calibration.rs（verified門檻、非單調與人工分類）、dxgi.rs（僅此模組有unsafe／影格釋放與Map生命周期）、win.rs（退出／部分資料、PresentMon session、結果）；crates/core/src/lib.rs（run_trial／per-action timing／到期釋放）；crates/win-platform/src/windows_impl/runtime.rs（固定trial佇列／spin停止检查）、managed.rs（calibration只disarm Hook）、supervise.rs（session結果分派）。

## 提交

f6d2473：測試契約；b925812：量測與校準實作；第三筆：統計／停止／人工分類收尾、操作文件與Windows驗收（見git log）。
