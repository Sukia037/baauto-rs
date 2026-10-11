use super::*;
use baauto_config::{CardUiSide, HandSettings, Mode};
use rtrb::{Consumer, Producer, RingBuffer};
use std::{
    io::Write,
    mem::size_of,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering::SeqCst},
    },
    thread,
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{LibraryLoader::*, Threading::*},
        UI::{
            Input::Ime::{HIMC, ImmAssociateContext},
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, Result, w},
};
#[derive(Clone, Copy)]
struct Received {
    qpc: u64,
    message: u32,
    wparam: usize,
    lparam: isize,
    extra_info: usize,
}
struct ReceiverState {
    clock: Clock,
    hand: HandSettings,
    mode: Mode,
    records: Producer<Received>,
    received: u64,
    recent: [Option<Received>; 20],
    wake: Arc<Event>,
    lost: u64,
    down_count: u64,
}
unsafe extern "system" fn window(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if msg == WM_NCCREATE {
            // SAFETY: WM_NCCREATE supplies a live CREATESTRUCTW. Caller owns Box<ReceiverState> through the entire window lifetime.
            unsafe {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            }
            return Some(LRESULT(1));
        }
        // SAFETY: HWND belongs to this class; stored pointer is valid until the message loop terminates and owned Box is dropped.
        let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut ReceiverState;
        if ptr.is_null() {
            return None;
        }
        // SAFETY: All window messages are dispatched serially on the receiver thread; no alias is accessed concurrently.
        let state = unsafe { &mut *ptr };
        if matches!(
            msg,
            WM_KEYDOWN
                | WM_KEYUP
                | WM_SYSKEYDOWN
                | WM_SYSKEYUP
                | WM_LBUTTONDOWN
                | WM_LBUTTONUP
                | WM_MOUSEMOVE
        ) {
            if matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN | WM_LBUTTONDOWN) {
                state.down_count = state.down_count.wrapping_add(1);
            }
            let record = Received {
                qpc: state.clock.now().0,
                message: msg,
                wparam: wparam.0,
                lparam: lparam.0,
                // SAFETY: Value-only lookup of extra information for this receiver thread's current message.
                extra_info: unsafe { GetMessageExtraInfo().0 as usize },
            };
            state.recent[(state.received % 20) as usize] = Some(record);
            state.received = state.received.saturating_add(1);
            if state.records.push(record).is_err() {
                state.lost = state.lost.saturating_add(1);
            }
            let _ = state.wake.set();
            // SAFETY: Invalidates only this live window; no RECT pointer is passed.
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return if matches!(msg, WM_SYSKEYDOWN | WM_SYSKEYUP) {
                None
            } else {
                Some(LRESULT(0))
            };
        }
        if msg == WM_PAINT {
            // SAFETY: PAINTSTRUCT and RECT are valid stack output storage; every BeginPaint is paired with EndPaint in this block.
            unsafe {
                let mut paint = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut paint);
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let background = CreateSolidBrush(if state.down_count % 2 == 0 {
                    COLORREF(0x00eeeeee)
                } else {
                    COLORREF(0x00aa5533)
                });
                let _ = FillRect(dc, &client, background);
                let _ = DeleteObject(HGDIOBJ(background.0));
                SetBkMode(dc, TRANSPARENT);
                let rect = state.hand.layout(state.mode);
                let mut origin = POINT::default();
                let _ = ClientToScreen(hwnd, &mut origin);
                let mut info = MONITORINFO {
                    cbSize: size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                let _ =
                    GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info);
                let width = info.rcMonitor.right - info.rcMonitor.left;
                let height = info.rcMonitor.bottom - info.rcMonitor.top;
                let x0 = info.rcMonitor.left - origin.x + (rect.x[0] * f64::from(width)) as i32;
                let x1 = info.rcMonitor.left - origin.x + (rect.x[1] * f64::from(width)) as i32;
                let y0 = info.rcMonitor.top - origin.y + (rect.y[0] * f64::from(height)) as i32;
                let y1 = info.rcMonitor.top - origin.y + (rect.y[1] * f64::from(height)) as i32;
                let pen = CreatePen(PS_SOLID, 2, COLORREF(0x00aa6600));
                let old = SelectObject(dc, HGDIOBJ(pen.0));
                let brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
                let _ = Rectangle(dc, x0, y0, x1, y1);
                for i in 0..state.mode.slots() {
                    let x = x0 + (x1 - x0) * i32::from(i) / i32::from(state.mode.slots());
                    let _ = MoveToEx(dc, x, y0, None);
                    let _ = LineTo(dc, x, y1);
                    let text = format!("{}", i + 1).encode_utf16().collect::<Vec<_>>();
                    let _ = TextOutW(dc, x + 10, y0 + 10, &text);
                }
                SelectObject(dc, old);
                SelectObject(dc, brush);
                let _ = DeleteObject(HGDIOBJ(pen.0));
                let header = format!(
                    "BAauto Receiver | {:?} {:?} | events={} lost={} | Alt+F4 to close",
                    state.mode, state.hand.card_ui_side, state.received, state.lost
                )
                .encode_utf16()
                .collect::<Vec<_>>();
                let _ = TextOutW(dc, 20, 20, &header);
                for (i, r) in (0..state.received.min(20))
                    .filter_map(|offset| {
                        state.recent[((state.received - 1 - offset) % 20) as usize]
                    })
                    .enumerate()
                {
                    let text = format!(
                        "{}  msg=0x{:04x} key={} data={}",
                        r.qpc, r.message, r.wparam, r.lparam
                    )
                    .encode_utf16()
                    .collect::<Vec<_>>();
                    let _ = TextOutW(dc, 20, 60 + i as i32 * 22, &text);
                }
                let _ = EndPaint(hwnd, &paint);
            }
            return Some(LRESULT(0));
        }
        if msg == WM_DESTROY {
            // SAFETY: Ends only this thread's message loop. Owned receiver state remains live through loop exit.
            unsafe {
                PostQuitMessage(0);
            }
            return Some(LRESULT(0));
        }
        None
    }))
    .ok()
    .flatten();
    if let Some(result) = answer {
        result
    } else {
        // SAFETY: Forwards original parameters to Windows' default window procedure.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }
}
pub fn run_receiver(mode: Mode, side: CardUiSide, output: &Path) -> Result<()> {
    set_dpi_awareness()?;
    let (producer, consumer) = RingBuffer::new(8192);
    let wake = Arc::new(Event::new(false)?);
    let mut writer = ReceiverWriter::new(output, consumer, wake.clone())
        .map_err(|e| failure(&format!("receiver CSV: {e}")))?;
    let mut state = Box::new(ReceiverState {
        clock: Clock::new()?,
        hand: HandSettings {
            card_ui_side: side,
            ..Default::default()
        },
        mode,
        records: producer,
        received: 0,
        recent: [None; 20],
        wake,
        lost: 0,
        down_count: 0,
    });
    // SAFETY: Class and callbacks are static; Box lives until all windows have been destroyed and GetMessage returns.
    unsafe {
        let module = GetModuleHandleW(PCWSTR::null())?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window),
            hInstance: HINSTANCE(module.0),
            lpszClassName: w!("BAautoReceiver"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err(windows::core::Error::from_thread());
        }
        let r = info.rcMonitor;
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("BAautoReceiver"),
            w!("BAauto Receiver"),
            WS_POPUP | WS_THICKFRAME | WS_VISIBLE,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            None,
            None,
            Some(HINSTANCE(module.0)),
            Some((&mut *state as *mut ReceiverState).cast()),
        )?;
        let _window = ReceiverWindow(hwnd);
        // SAFETY: This thread owns the live receiver HWND. Disassociate only its
        // default IME context; the context remains OS-owned and is not destroyed.
        // The receiver records virtual keys, rather than composing text.
        let _ = ImmAssociateContext(hwnd, HIMC::default());
        let _ = SetForegroundWindow(hwnd);
        let mut msg = MSG::default();
        loop {
            let status = GetMessageW(&mut msg, None, 0, 0).0;
            if status == -1 {
                return Err(windows::core::Error::from_thread());
            }
            if status == 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    let count = writer
        .finish()
        .map_err(|e| failure(&format!("receiver CSV: {e}")))?;
    let meta = serde_json::json!({"qpc_frequency":state.clock.rate.0,"lost_records":state.lost,"written_records":count,"complete":true});
    std::fs::write(
        output.with_extension("json"),
        serde_json::to_vec_pretty(&meta).map_err(|_| failure("receiver metadata encode"))?,
    )
    .map_err(|_| failure("receiver metadata write"))?;
    println!("receiver lost events: {}", state.lost);
    Ok(())
}
struct ReceiverWindow(HWND);
impl Drop for ReceiverWindow {
    fn drop(&mut self) {
        // SAFETY: The receiver state Box outlives this guard. Check ownership because WM_CLOSE may already have destroyed the HWND.
        unsafe {
            let mut pid = 0;
            let tid = GetWindowThreadProcessId(self.0, Some(&mut pid));
            if pid == GetCurrentProcessId() && tid == GetCurrentThreadId() {
                let _ = DestroyWindow(self.0);
            }
        }
    }
}
struct ReceiverWriter {
    done: Arc<AtomicBool>,
    wake: Arc<Event>,
    join: Option<thread::JoinHandle<std::io::Result<u64>>>,
}
impl ReceiverWriter {
    fn new(
        path: &Path,
        mut consumer: Consumer<Received>,
        wake: Arc<Event>,
    ) -> std::io::Result<Self> {
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        writeln!(file, "qpc,message,wparam,lparam,extra_info")?;
        file.flush()?;
        let done = Arc::new(AtomicBool::new(false));
        let stop = done.clone();
        let signal = wake.clone();
        let join = thread::spawn(move || {
            let mut count = 0_u64;
            let clock = Clock::new().map_err(|e| std::io::Error::other(e.to_string()))?;
            let mut flushed = clock.now();
            loop {
                while let Ok(r) = consumer.pop() {
                    writeln!(
                        file,
                        "{},{},{},{},{}",
                        r.qpc, r.message, r.wparam, r.lparam, r.extra_info
                    )?;
                    count = count.saturating_add(1);
                }
                if stop.load(SeqCst) && consumer.is_empty() {
                    break;
                }
                if clock.now().0.saturating_sub(flushed.0) >= clock.rate.0 {
                    file.flush()?;
                    flushed = clock.now();
                }
                // SAFETY: The shared auto-reset Event remains owned by signal; waiting occurs only on this file worker.
                if unsafe { WaitForSingleObject(signal.raw(), 50) } == WAIT_FAILED {
                    return Err(std::io::Error::last_os_error());
                }
            }
            file.flush()?;
            file.get_ref().sync_all()?;
            Ok(count)
        });
        Ok(Self {
            done,
            wake,
            join: Some(join),
        })
    }
    fn finish(&mut self) -> std::io::Result<u64> {
        self.done.store(true, SeqCst);
        let _ = self.wake.set();
        self.join.take().map_or(Ok(0), |j| {
            j.join()
                .map_err(|_| std::io::Error::other("receiver writer panic"))?
        })
    }
}
impl Drop for ReceiverWriter {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
