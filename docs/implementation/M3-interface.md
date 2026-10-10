# M3 介面

`protocol::ipc`：有界長度前綴 JSON 訊框、Hello 版本握手、CompiledWire（背景編碼／驗證後還原固定觸發表）、每個命令的 cmd_seq、每個回覆的 session。

`protocol::supervisor`：純狀態機，輸入為握手／設定／實體放開／Arm／停止／健康／程序退出；固定容量 effect 清單。復原釋放只在收到 ProcessExited 後產生。Faulted 必須新的明確 RequestArm，不沿用動作。Windows 執行 effect 的監督執行緒不執行檔案 I/O 或阻塞的 IPC。

Windows 層：CreateProcessW＋DuplicateHandle、overlapped named pipe 封裝、緊急熱鍵與 Raw Input 隱藏視窗、停止與退出證明、managed runtime。每個 Arm 新建 Event；首次安裝与換設定都在 Disarmed，舊 cfg 在 Control 釋放。
