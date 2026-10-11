//! Independent supervisor message pump. Raw Input is evidence only, never a macro trigger.
use super::*;
use baauto_config::{Chord, ModsMatch};
use std::{marker::PhantomData, mem::size_of, rc::Rc};
use windows::{
    Win32::{
        Foundation::*,
        System::LibraryLoader::*,
        UI::{
            Input::{KeyboardAndMouse::*, *},
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, Result, w},
};
struct HealthState {
    related: [bool; 256],
    target: usize,
    raw: u64,
    stop: bool,
    record: u64,
}
unsafe extern "system" fn window(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let result = std::panic::catch_unwind(|| {
        if msg == WM_NCCREATE {
            // SAFETY: Windows supplies live CREATESTRUCTW; creator keeps Box<HealthState> alive through DestroyWindow.
            unsafe {
                let cs = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            }
            return;
        }
        // SAFETY: Userdata belongs to this window class, is set before messages, and callbacks run only on the owning supervisor thread.
        unsafe {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut HealthState;
            if ptr.is_null() {
                return;
            }
            let state = &mut *ptr;
            if msg == WM_HOTKEY {
                if wparam.0 == 1 {
                    state.stop = true;
                } else if wparam.0 == 2 {
                    state.record += 1;
                }
            }
            if msg == WM_INPUT && foreground_is(state.target) {
                let mut raw = RAWINPUT::default();
                let mut size = size_of::<RAWINPUT>() as u32;
                if GetRawInputData(
                    HRAWINPUT(lparam.0 as *mut _),
                    RID_INPUT,
                    Some((&mut raw as *mut RAWINPUT).cast()),
                    &mut size,
                    size_of::<RAWINPUTHEADER>() as u32,
                ) == u32::MAX
                {
                    return;
                }
                if raw.header.hDevice.0.is_null() {
                    return;
                }
                let bound = if raw.header.dwType == RIM_TYPEKEYBOARD.0 {
                    let key = raw.data.keyboard.VKey as usize;
                    key < 256 && state.related[key]
                } else if raw.header.dwType == RIM_TYPEMOUSE.0 {
                    let flags = raw.data.mouse.Anonymous.Anonymous.usButtonFlags;
                    [
                        (1u16, 1usize),
                        (2, 1),
                        (4, 2),
                        (8, 2),
                        (16, 4),
                        (32, 4),
                        (64, 5),
                        (128, 5),
                        (256, 6),
                        (512, 6),
                    ]
                    .iter()
                    .any(|(bit, key)| flags & bit != 0 && state.related[*key])
                } else {
                    false
                };
                if bound {
                    let count = if raw.header.dwType == RIM_TYPEMOUSE.0 {
                        let flags = raw.data.mouse.Anonymous.Anonymous.usButtonFlags;
                        [
                            (1u16, 1usize),
                            (2, 1),
                            (4, 2),
                            (8, 2),
                            (16, 4),
                            (32, 4),
                            (64, 5),
                            (128, 5),
                            (256, 6),
                            (512, 6),
                        ]
                        .iter()
                        .filter(|(bit, key)| flags & bit != 0 && state.related[*key])
                        .count() as u64
                    } else {
                        1
                    };
                    state.raw = state.raw.saturating_add(count);
                }
            }
        }
    });
    if result.is_err() {
        // SAFETY: Same owning thread and live userdata; panic conservatively requests a stop.
        unsafe {
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut HealthState;
            if !p.is_null() {
                (*p).stop = true;
            }
        }
    }
    // SAFETY: Default processing is necessary to release foreground WM_INPUT storage and handle window lifecycle.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
pub struct HealthWindow {
    hwnd: HWND,
    state: Box<HealthState>,
    _owner: PhantomData<Rc<()>>,
    stop_hotkey: Chord,
    record_hotkey: Option<Chord>,
}
fn modifiers(chord: Chord) -> HOT_KEY_MODIFIERS {
    let mask = match chord.mods {
        ModsMatch::AtLeast(m) | ModsMatch::Exact(m) => m,
    };
    MOD_NOREPEAT
        | if mask & 1 != 0 {
            MOD_CONTROL
        } else {
            HOT_KEY_MODIFIERS(0)
        }
        | if mask & 2 != 0 {
            MOD_SHIFT
        } else {
            HOT_KEY_MODIFIERS(0)
        }
        | if mask & 4 != 0 {
            MOD_ALT
        } else {
            HOT_KEY_MODIFIERS(0)
        }
        | if mask & 8 != 0 {
            MOD_WIN
        } else {
            HOT_KEY_MODIFIERS(0)
        }
}
impl HealthWindow {
    pub fn new(related: [bool; 256], stop: Chord, record: Option<Chord>) -> Result<Self> {
        let mut state = Box::new(HealthState {
            related,
            target: 0,
            raw: 0,
            stop: false,
            record: 0,
        });
        // SAFETY: Static class callback, stable Box pointer, message-only hidden window owned by this supervisor thread.
        unsafe {
            let module = GetModuleHandleW(PCWSTR::null())?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window),
                hInstance: HINSTANCE(module.0),
                lpszClassName: w!("BAautoSupervisorHealth"),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err(windows::core::Error::from_thread());
            }
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BAautoSupervisorHealth"),
                w!("BAauto Supervisor"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(HINSTANCE(module.0)),
                Some((&mut *state as *mut HealthState).cast()),
            )?;
            let window = Self {
                hwnd,
                state,
                _owner: PhantomData,
                stop_hotkey: stop,
                record_hotkey: record,
            };
            RegisterHotKey(Some(hwnd), 1, modifiers(stop), u32::from(stop.key.0))?;
            if let Some(record) = record {
                RegisterHotKey(Some(hwnd), 2, modifiers(record), u32::from(record.key.0))?;
            }
            RegisterRawInputDevices(
                &[
                    RAWINPUTDEVICE {
                        usUsagePage: 1,
                        usUsage: 6,
                        dwFlags: RIDEV_INPUTSINK,
                        hwndTarget: hwnd,
                    },
                    RAWINPUTDEVICE {
                        usUsagePage: 1,
                        usUsage: 2,
                        dwFlags: RIDEV_INPUTSINK,
                        hwndTarget: hwnd,
                    },
                ],
                size_of::<RAWINPUTDEVICE>() as u32,
            )?;
            Ok(window)
        }
    }
    pub fn rebind(&mut self, stop: Chord, record: Option<Chord>) -> Result<()> {
        // SAFETY: Called only on the owner supervisor thread after cooperative stop. Failure leaves readiness blocked; old bindings are restored where possible.
        unsafe {
            let _ = UnregisterHotKey(Some(self.hwnd), 1);
            let _ = UnregisterHotKey(Some(self.hwnd), 2);
            let result = RegisterHotKey(Some(self.hwnd), 1, modifiers(stop), u32::from(stop.key.0))
                .and_then(|_| {
                    if let Some(record) = record {
                        RegisterHotKey(
                            Some(self.hwnd),
                            2,
                            modifiers(record),
                            u32::from(record.key.0),
                        )
                    } else {
                        Ok(())
                    }
                });
            if let Err(error) = result {
                let _ = UnregisterHotKey(Some(self.hwnd), 1);
                let _ = UnregisterHotKey(Some(self.hwnd), 2);
                let _ = RegisterHotKey(
                    Some(self.hwnd),
                    1,
                    modifiers(self.stop_hotkey),
                    u32::from(self.stop_hotkey.key.0),
                );
                if let Some(record) = self.record_hotkey {
                    let _ = RegisterHotKey(
                        Some(self.hwnd),
                        2,
                        modifiers(record),
                        u32::from(record.key.0),
                    );
                }
                return Err(error);
            }
        }
        self.stop_hotkey = stop;
        self.record_hotkey = record;
        Ok(())
    }
    pub fn set_target(&mut self, hwnd: usize) {
        self.state.target = hwnd;
    }
    pub fn set_related(&mut self, related: [bool; 256]) {
        self.state.related = related;
    }
    pub fn raw_count(&self) -> u64 {
        self.state.raw
    }
    pub fn take_stop(&mut self) -> bool {
        std::mem::take(&mut self.state.stop)
    }
    pub fn record_count(&self) -> u64 {
        self.state.record
    }
    pub fn pump(&self) {
        // SAFETY: Only messages for this thread are dispatched; bounded work prevents input floods from starving health/stop checks.
        unsafe {
            let mut msg = MSG::default();
            for _ in 0..256 {
                if !PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    pub(crate) fn wait(&self, handles: &[HANDLE], timeout: u32) -> Result<()> {
        // SAFETY: Every handle is owned and remains live through this call; waits also wake on this supervisor thread's messages.
        let result = unsafe {
            MsgWaitForMultipleObjectsEx(Some(handles), timeout, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
        };
        if result == WAIT_FAILED {
            Err(windows::core::Error::from_thread())
        } else {
            Ok(())
        }
    }
}
impl Drop for HealthWindow {
    fn drop(&mut self) {
        // SAFETY: Only owner thread drops the window, before Box<HealthState>. No callback outlives DestroyWindow.
        unsafe {
            let _ = UnregisterHotKey(Some(self.hwnd), 1);
            let _ = UnregisterHotKey(Some(self.hwnd), 2);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
