# M1 公開介面與驗收合約

- config：Key／Chord、PointSet、KeysProfile、HandSettings、TimingProfile、EngineParams；讀寫分離的 JSON 模型；compile_with_params → CompiledConfig，包含固定觸發表與 tick 時序。
- core：Ticks／TickRate／SessionId／PressId／ActionId、InputEvent、Batch、SubmitOutcome、Engine；on_input → next_batch → commit；next_wake、held、reports、progress。
- core::sim：FakeClock、FakeDriver、Scenario → CSV。Windows 不進入 M1。
- 驗收名稱先建立於兩個 acceptance.rs，後續提交逐一替換為可執行斷言；最終不得留 ignored／todo 合約。
- 時序與工程容量是不同設定；Engine 在初始化配置固定槽位，Arm 後不配置。
