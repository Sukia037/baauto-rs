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
                if (1..=16).contains(&wparam.0) {
                    state.stop = true;
                } else if (17..=32).contains(&wparam.0) {
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
            register_group(hwnd, 1, stop)?;
            if let Some(record) = record {
                register_group(hwnd, 17, record)?;
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
        unregister_groups(self.hwnd);
        let result = register_group(self.hwnd, 1, stop)
            .and_then(|_| record.map_or(Ok(()), |r| register_group(self.hwnd, 17, r)));
        if let Err(error) = result {
            unregister_groups(self.hwnd);
            let _ = register_group(self.hwnd, 1, self.stop_hotkey);
            if let Some(record) = self.record_hotkey {
                let _ = register_group(self.hwnd, 17, record);
            }
            return Err(error);
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
            unregister_groups(self.hwnd);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn register_group(hwnd: HWND, base: i32, chord: Chord) -> Result<()> {
    if chord.key.is_mouse() {
        return Err(failure("全域停止／錄點需要鍵盤熱鍵"));
    }
    let masks = crate::hotkeys::global_hotkey_masks(chord);
    if masks.is_empty() {
        return Err(failure("全域熱鍵修飾符無效"));
    }
    for mask in masks {
        let exact = Chord {
            mods: ModsMatch::Exact(mask),
            ..chord
        };
        // SAFETY: Live window owned by this thread, unique reserved IDs, value-only key and modifier arguments.
        unsafe {
            RegisterHotKey(
                Some(hwnd),
                base + i32::from(mask),
                modifiers(exact),
                u32::from(chord.key.0),
            )
            .map_err(|_| {
                failure(&format!(
                    "全域熱鍵 {exact} 註冊失敗；請改鍵或關閉佔用該組合的程式"
                ))
            })?;
        }
    }
    Ok(())
}
fn unregister_groups(hwnd: HWND) {
    for id in 1..=32 {
        // SAFETY: Only this HealthWindow owns these IDs. Unregistered IDs are harmless failures.
        unsafe {
            let _ = UnregisterHotKey(Some(hwnd), id);
        }
    }
}
