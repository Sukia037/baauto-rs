# M5 量測與校準

狀態：工具與可攜測試完成；Windows 原生、PresentMon、DXGI 和真實遊戲驗收尚未執行。所有目前範本仍是 `verified: false`。請在 Windows 的非排名練習內容操作，採遊戲所在整個 16:9 螢幕；任務檔的視窗化要求和 v0.4 §10 衝突，這裡依規格使用全螢幕或覆蓋整個螢幕的無邊框，不做內容區補償。

## 建置與接收器

```powershell
cargo build --release --workspace
./target/release/baauto-receiver.exe --output receiver.csv
./target/release/baauto-bench.exe measure --points tests/fixtures/m2-basic.json --keys tests/fixtures/m2-keys.json --out measure-01 --seconds 60 --presentmon C:/tools/PresentMon.exe
```

新建輸出目錄不能已存在。啟動後切回接收器，Arm 最多等 30 秒；操作 W、z、2，交錯按放，量測結束走停止協定。`ring.csv` 保留原始 QPC；`software.json/md` 有鉤子處理、喚醒、晚醒、注入、停止、短送與遺失筆數。P99 要 1,000 筆、P99.9 要 10,000 筆，不足顯示「樣本不足」。停止採共享記憶體第一個 `stop_request_qpc`；來源未發布該時間時回退引擎觀察時間，不能拿回退值宣稱完整端到端停止上界。

PresentMon 使用獨立具名 session、`--qpc_time`、預設 metrics；要求整數 `CPUStartQPC`、`MsBetweenPresents`、`MsUntilDisplayed`，不接受 `--qpc_time_ms`。欄位／參數依 [PresentMon 官方文件](https://github.com/GameTechDev/PresentMon/blob/main/README-ConsoleApplication.md)。

```powershell
./target/release/baauto-bench.exe align measure-01/presentmon.csv measure-01/ring.csv aligned.csv
./target/release/baauto-bench.exe analyze measure-01/ring.csv 10000000 imported.md
```

analyze 的頻率請用 measurement.json 的實際值，上面的 10000000 僅是指令示例。落在哪幀是 send_end **之後**第一個 CPUStartQPC；不是遊戲已接受輸入的證明。DXGI 只使用 LastPresentTime 非零的桌面影格，忽略單純游標更新；API 語意見 [AcquireNextFrame](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgioutputduplication-acquirenextframe) 與 [FRAME_INFO](https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ns-dxgi1_2-dxgi_outdupl_frame_info)。畫面受保護、旋轉、AccessLost、失焦、引擎退出會中止，不能沿用未完成資料為 verified；切換顯示模式後重新開始一份計畫。

## 先承諾計畫

```powershell
./target/release/baauto-bench.exe plan draft.json
# 手改 draft.json：實際 paths、模式、動作、ROI、粗掃、細掃、次數、成功／偵測門檻、條件、遊戲版本、解析度、更新率。
./target/release/baauto-bench.exe seal draft.json calib-plan.json settings.json
```

seal 以實際點位／按鍵／手牌內容做 SHA-256；scan 在 Arm 前保存計畫 JSON 與 hash。試驗期間改來源或事後改門檻，結果不能套用。範例 EXAMPLE 身分必須換成實際環境。六人／十人分別建計畫，單一參數校準不能推廣到其他動作／模式；profile identity 明記動作、參數、模式與 hash。

統計採雙側 Wilson；零失敗要達 99.5%、95% 信心水準，工具算 **765** 次。任務的約 600 是近似 rule of three，不是此雙側區間的答案；方法參考 [NIST Wilson interval](https://www.itl.nist.gov/div898/handbook/prc/section2/prc241.htm)。預設 1,000 次並不保證有足夠第 1 層 P99.9 樣本，必要時提高預先承諾次數。

## 先驗證偵測器，再掃描

```powershell
./target/release/baauto-bench.exe scan --plan calib-plan.json --settings settings.json --target-exe BlueArchive.exe --out pilot-01 --pilot-count 100 --presentmon C:/tools/PresentMon.exe
```

pilot 匯出每筆 ROI 前後 BMP 和 trials.csv。獨立查看截圖／錄影，只修改 CSV `label`：Valid、Missed、Misoperation、WrongOrder、Duplicate、Ineligible（冷卻／資源不足）。保留 detected 和 QPC 原值。平均畫面差異只能觀察變化，不能自行區分順序錯或重複，因此人工作業不能省略。若前後兩張無法判斷順序，使用 240fps 錄影逐筆核對。足夠多的獨立標註才能讓 Wilson 誤判率上界低於計畫門檻。

```powershell
./target/release/baauto-bench.exe validate-detector calib-plan.json labelled-pilot.csv detector.json
./target/release/baauto-bench.exe scan --plan calib-plan.json --settings settings.json --target-exe BlueArchive.exe --out scan-01 --validation detector.json --presentmon C:/tools/PresentMon.exe
```

粗掃後對每一個成功率變化邊界細掃，保留多段有效範圍。每筆均記錄 seed 產生的 0 到一幀均勻相位，另有控制程序送命令的 100ms 提前量，不當成遊戲時序參數。每筆完成立即 sync trials.csv；中斷保留已完成列與計畫，不產生 verified profile。RunTrial 只在 Calibration session，Hook 不觸發一般巨集，輸出仍經 Engine／帳本／Driver／停止協定。

掃描仍保留尚未人工分類的結果，`outcomes_sha256: null`；不能直接 apply。對完整 trials.csv 逐筆標記，Ineligible 排除，其他四種失敗保留。排除後有效次數不足計畫的點不能驗證；請預先增加次數或重新承諾計畫重跑，不能降低門檻。

```powershell
./target/release/baauto-bench.exe finalize calib-plan.json scan-01/result.json labelled-scan.csv final-result.json
./target/release/baauto-bench.exe apply calib-plan.json final-result.json
```

finalize 保存標註 hash、重新計算樣本與安全選值；只有成功率下界、完整粗掃、身分、門檻、偵測器、人工作業、實測 PresentMon FPS、P99.9 晚醒餘裕、高解析度 timer、無遺失全部通過才 verified。`escape_gap` 使用選值到選值＋餘裕的合法提交範圍。apply 檢查原按鍵檔 hash，經 M4 原子存檔先備份；不要在另一個 app 已啟用時直接覆寫其檔案，先停止再載入新檔。

## A/B 與情境

```powershell
cargo build --release --workspace --features bench-experiments,test-hooks
./target/release/baauto-bench.exe ab --points tests/fixtures/m2-basic.json --keys tests/fixtures/m2-keys.json --out ab-01 --seconds 120 --presentmon C:/tools/PresentMon.exe --exercise-receiver --spin-us 100
```

八種組合各有獨立引擎、具名 PresentMon 和報告；test-hooks 只能對 baauto-receiver.exe 自動送 W。A/B 預設每組另採 1,000 次 idle Arm/Stop（可用 `--stop-cycles` 指定），報表保守比較全部尾延遲／幀 P99／CPU／停止 P99／遺失／失聯；缺值不能保留實驗。idle 停止樣本仍需補實際多動作負載停止，不能推廣成滿載結論。目前沒有原生 A/B 數據，因此維持 Hook TIME_CRITICAL、Engine NORMAL、自旋 0。禁止 REALTIME、CPU 綁定。

以下各情境各跑一份新目錄並保存負載工具／版本／時間：閒置、GPU 負載、CPU 負載、多鍵交錯、焦點切换、移動視窗、GUI 操作、換設定、故意讓紀錄讀取端落後、输入洪流、睡眠恢復。只有接收器流程可以注入測試輸入；遊戲使用實體操作。

## Windows 驗收

`tests/windows/M5.ps1 -Interactive -PresentMon C:/tools/PresentMon.exe` 建置與執行接收器／DXGI／量測／對齊／A/B。保留接收器 CSV，核對首次畫面變化 LastPresentTime 不早於接收器相應按鍵 QPC，輸入到畫面用差值／QPC 頻率計算。另逐格測六人3／十人5個中心、縫與邊界；普通點位、repeat、牌鍵與停止交錯依 M2 接收器驗收。

- [ ] 接收器量測與報告完整、遺失／短送可解釋。
- [ ] DXGI 顏色變化和接收器 QPC 相符。
- [ ] PresentMon QPC 對齊正確。
- [ ] 八種 A/B 完成，資料量達門檻且追加滿載停止。
- [ ] 偵測器混淆矩陣通過。
- [ ] 非排名內容至少一個動作的完整校準，finalize/apply 真正得到 verified。

以上原生項目目前全部待實跑，沒有用合成資料打勾。
