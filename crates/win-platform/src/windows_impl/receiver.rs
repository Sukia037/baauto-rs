use super::*;
use baauto_config::{CardUiSide, HandSettings, Mode};
use std::{mem::size_of, path::Path};
use windows::{
    Win32::{
        Foundation::*, Graphics::Gdi::*, System::LibraryLoader::*, UI::WindowsAndMessaging::*,
    },
    core::{PCWSTR, Result, w},
};
#[derive(Clone, Copy)]
struct Received {
    qpc: u64,
    message: u32,
    wparam: usize,
    lparam: isize,
}
struct ReceiverState {
    clock: Clock,
    hand: HandSettings,
    mode: Mode,
    records: Vec<Received>,
    lost: u64,
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
            let record = Received {
                qpc: state.clock.now().0,
                message: msg,
                wparam: wparam.0,
                lparam: lparam.0,
            };
            if state.records.len() < state.records.capacity() {
                state.records.push(record);
            } else {
                state.lost += 1;
            }
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
                let _ = FillRect(dc, &client, HBRUSH((COLOR_WINDOW.0 + 1) as *mut _));
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
                    state.mode,
                    state.hand.card_ui_side,
                    state.records.len(),
                    state.lost
                )
                .encode_utf16()
                .collect::<Vec<_>>();
                let _ = TextOutW(dc, 20, 20, &header);
                for (i, r) in state.records.iter().rev().take(20).enumerate() {
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
    let _ = set_dpi_awareness();
    let mut state = Box::new(ReceiverState {
        clock: Clock::new()?,
        hand: HandSettings {
            card_ui_side: side,
            ..Default::default()
        },
        mode,
        records: Vec::with_capacity(1_000_000),
        lost: 0,
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
    use std::io::Write;
    let mut file =
        std::fs::File::create(output).map_err(|_| failure("receiver CSV open failed"))?;
    writeln!(file, "qpc,message,wparam,lparam")
        .map_err(|_| failure("receiver CSV write failed"))?;
    for r in &state.records {
        writeln!(file, "{},{},{},{}", r.qpc, r.message, r.wparam, r.lparam)
            .map_err(|_| failure("receiver CSV write failed"))?;
    }
    println!("receiver lost events: {}", state.lost);
    Ok(())
}
