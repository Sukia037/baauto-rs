# M2 介面

protocol::shm：SharedMemory / Header / Progress / Ledger / Ring；固定 repr(C) 原子版面，控制端建立、引擎寫入；RingReader 檢測覆寫與不一致紀錄。
win-platform::hook：HookState、RawEvent → Decision（固定 ArrayVec）；先做純邏輯測試。
win-platform::geometry：螢幕與虛擬桌面、像素與 SendInput absolute 換算；負原點與邊緣必須精確。
Windows 安全封裝：Event、Timer、Clock、Mapping、Driver、HookThread；Engine 唯一正常輸出擁有者。receiver 與 engine 二進位透過安全 API 進入平台。
