# M1 實作回報

## 實作與验收

config 提供兩種 JSON、23 個 preset、HandSettings、可讀 Chord、衝突分析、固定觸發表與向上取整的 tick 編譯。core 提供六種動作、按鍵與滑鼠／Esc 所有權、session／press／generation、两阶段 batch/commit、停止与故障、健康期限与 sim CSV。

M1 任務所有具名驗收都有可執行測試：15 個設定測試、38 個核心測試（含額外回歸與兩個 proptest）。零配置測試跑 100,000 輪 Tap；三個熱路徑函式合計配置為 0。沒有留 ignored／todo 合約。一般 Repeat 逐輪公平仲裁；清理只操作該 action 持有的鍵。

## 驗證

- macOS / Rust stable 1.99.0：cargo fmt --all --check、cargo clippy --workspace --all-targets -- -D warnings、cargo test --workspace 通過；53 個測試。
- Windows 交叉編譯檢查加入 CI；後續 Windows 層的各次結果另列。
- CI 設 Linux 自動測試及 Windows 原生自動測試；推送不代表 CI 已通過，必須查看實際結果。
- 實際 Windows Hook／SendInput 與遊戲時序不在 M1 範圍；沒有遊戲校準數據。

## 介面與決策

事件使用 InputEvent envelope + InputKind，所有輸入均帶 session/config/geometry/seq；避免草案 Focus 沒有 session 的漏洞。TriggerRepeat 帶當下 cursor，牌鍵 FollowTypematic 能重新判定格。增加 NotSubmitted 表示平台在注入前發現停止，避免將未送出批次當短送。

Batch 固定容量 257，只停止釋放可用滿容量；正常批次 1–2 筆。TimingProfile 的 allowed_lateness_ms 是按動作階段具名表；null 代表允許晚送，釋放階段永不因逾期丟棄。所有範本時間為未驗證來源值，沒有引擎硬編碼的遊戲間隔。

新增依賴 serde_path_to_error（巢狀錯誤路徑）、allocation-counter（安全零配置測試）；原因已記 docs/decisions.md。Task 草案與正式規格的 Repeat 仲裁、停止批次容量差異也已記錄，採正式規格。

## 複查位置

- crates/core/src/lib.rs：Engine::start / cancel / next_batch / commit / next_wake；尤其資源等待、in-flight stop、短送帳本。
- crates/config/src/compile.rs：analyze_conflicts、ticks_ms、compile_with_params；容量、時間與 AtLeast/Exact 的重疊。
- crates/core/tests/acceptance.rs、acceptance.proptest-regressions：隨機失敗案例、晚醒及故障回歸。
- 初始合約提交 c20986b，核心實作提交 860405f；驗收提交請看 git log --grep='(M1)'。
