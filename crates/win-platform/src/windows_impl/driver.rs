use super::{Clock, Event, Mapping, virtual_desktop};
use crate::geometry::{Rect, absolute_for_pixel};
use baauto_core::*;
use std::{marker::PhantomData, mem::size_of, rc::Rc, sync::Arc};
use windows::Win32::{
    Foundation::HWND,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::GetForegroundWindow},
};

/// Constructed and used on the Engine thread. Rc phantom makes the normal Driver !Send and !Sync.
pub struct Driver {
    inputs: [INPUT; 257],
    marker: usize,
    stop: Arc<Event>,
    mapping: Arc<Mapping>,
    target: usize,
    monitor: Rect,
    desktop: Rect,
    clock: Clock,
    _owner: PhantomData<Rc<()>>,
    #[cfg(feature = "fault-injection")]
    short_next: bool,
}
impl Driver {
    pub fn new(
        marker: usize,
        stop: Arc<Event>,
        mapping: Arc<Mapping>,
        target: usize,
        monitor: Rect,
        clock: Clock,
    ) -> Self {
        Self {
            inputs: [INPUT::default(); 257],
            marker,
            stop,
            mapping,
            target,
            monitor,
            desktop: virtual_desktop(),
            clock,
            _owner: PhantomData,
            #[cfg(feature = "fault-injection")]
            short_next: false,
        }
    }
    pub(crate) fn cleanup_once(&mut self, session: SessionId) {
        let Some(held) = self.mapping.data().ledger.snapshot() else {
            return;
        };
        let mut events = arrayvec::ArrayVec::new();
        for i in 0..256 {
            if held.keys[i / 64] & (1 << (i % 64)) != 0 {
                events.push(OutEvent::KeyUp(Key(i as u8)));
            }
        }
        if held.mouse & 1 != 0 {
            events.push(OutEvent::LeftUp);
        }
        if !events.is_empty() {
            let b = Batch {
                id: 0,
                session,
                action: None,
                stage: 0,
                deadline: self.clock.now(),
                events,
            };
            let _ = self.submit(&b);
        }
    }
    #[cfg(feature = "fault-injection")]
    pub(crate) fn inject_short(&mut self) {
        self.short_next = true;
    }
    pub fn submit(&mut self, b: &Batch) -> SubmitOutcome {
        let releases_only = b
            .events
            .iter()
            .all(|e| matches!(e, OutEvent::KeyUp(_) | OutEvent::LeftUp));
        // SAFETY: GetForegroundWindow returns a borrowed HWND; comparing handles does not dereference it.
        if !releases_only
            && (self.stop.signaled() || unsafe { GetForegroundWindow() }.0 as usize != self.target)
        {
            return SubmitOutcome::NotSubmitted;
        }
        for (i, event) in b.events.iter().enumerate() {
            self.inputs[i] = match *event {
                OutEvent::KeyDown(k) | OutEvent::KeyUp(k) => {
                    // SAFETY: MapVirtualKeyW is a pure key-code lookup with no retained pointers.
                    let scan = unsafe { MapVirtualKeyW(u32::from(k.0), MAPVK_VK_TO_VSC) } as u16;
                    INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: VIRTUAL_KEY(u16::from(k.0)),
                                wScan: scan,
                                dwFlags: if matches!(event, OutEvent::KeyUp(_)) {
                                    KEYEVENTF_KEYUP
                                } else {
                                    KEYBD_EVENT_FLAGS(0)
                                },
                                time: 0,
                                dwExtraInfo: self.marker,
                            },
                        },
                    }
                }
                OutEvent::LeftDown | OutEvent::LeftUp => INPUT {
                    r#type: INPUT_MOUSE,
                    Anonymous: INPUT_0 {
                        mi: MOUSEINPUT {
                            dwFlags: if matches!(event, OutEvent::LeftDown) {
                                MOUSEEVENTF_LEFTDOWN
                            } else {
                                MOUSEEVENTF_LEFTUP
                            },
                            dwExtraInfo: self.marker,
                            ..Default::default()
                        },
                    },
                },
                OutEvent::MoveTo(p) => {
                    let Some((x, y)) = self
                        .monitor
                        .pixel(p)
                        .and_then(|(x, y)| absolute_for_pixel(self.desktop, x, y))
                    else {
                        return SubmitOutcome::NotSubmitted;
                    };
                    INPUT {
                        r#type: INPUT_MOUSE,
                        Anonymous: INPUT_0 {
                            mi: MOUSEINPUT {
                                dx: x,
                                dy: y,
                                dwFlags: MOUSEEVENTF_MOVE
                                    | MOUSEEVENTF_ABSOLUTE
                                    | MOUSEEVENTF_VIRTUALDESK,
                                dwExtraInfo: self.marker,
                                ..Default::default()
                            },
                        },
                    }
                }
            };
        }
        let physical = std::array::from_fn(|i| {
            self.mapping.data().progress.physical_keys[i].load(std::sync::atomic::Ordering::SeqCst)
        });
        if crate::hook::outputs_conflict(&b.events, physical) {
            self.mapping
                .data()
                .progress
                .stop_reason
                .store(8, std::sync::atomic::Ordering::SeqCst);
            let _ = self.stop.set();
            return SubmitOutcome::NotSubmitted;
        }
        self.mapping.data().ledger.before_send(&b.events);
        let begin = self.clock.now();
        // SAFETY: Fully initialized INPUT slice, exact C structure size, fixed valid buffer; every event carries our marker.
        #[cfg(feature = "fault-injection")]
        let count = if std::mem::take(&mut self.short_next) {
            b.events.len() / 2
        } else {
            b.events.len()
        };
        #[cfg(not(feature = "fault-injection"))]
        let count = b.events.len();
        // SAFETY: Slice is a bounded prefix of initialized INPUTs; fault builds may deliberately submit fewer events, while the ledger retains the full expected set.
        let inserted = unsafe { SendInput(&self.inputs[..count], size_of::<INPUT>() as i32) };
        let end = self.clock.now();
        if inserted == b.events.len() as u32 {
            self.mapping.data().ledger.after_success(&b.events);
            SubmitOutcome::Ok { begin, end }
        } else {
            SubmitOutcome::Short {
                inserted: inserted as u16,
                end,
            }
        }
    }
}
/// Supervisor recovery must supply proof from a signaled process handle, never merely TerminateProcess success.
pub struct ExitedProcessProof {
    pub(crate) _private: (),
}
#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct RecoveryReceipt {
    pub begin: u64,
    pub end: u64,
    pub inserted: u32,
    pub expected: u32,
}
impl RecoveryReceipt {
    pub fn success(self) -> bool {
        self.inserted == self.expected
    }
}
pub fn release_after_exit(
    proof: &ExitedProcessProof,
    snapshot: baauto_protocol::shm::LedgerSnapshot,
    marker: usize,
) -> bool {
    Clock::new()
        .is_ok_and(|clock| release_after_exit_recorded(proof, snapshot, marker, clock).success())
}
pub fn release_after_exit_recorded(
    _proof: &ExitedProcessProof,
    snapshot: baauto_protocol::shm::LedgerSnapshot,
    marker: usize,
    clock: Clock,
) -> RecoveryReceipt {
    let mut inputs = [INPUT::default(); 257];
    let mut n = 0;
    for i in 0..256 {
        if snapshot.keys[i / 64] & (1 << (i % 64)) != 0 {
            inputs[n] = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(i as u16),
                        dwFlags: KEYEVENTF_KEYUP,
                        dwExtraInfo: marker,
                        ..Default::default()
                    },
                },
            };
            n += 1;
        }
    }
    if snapshot.mouse & 1 != 0 {
        inputs[n] = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dwFlags: MOUSEEVENTF_LEFTUP,
                    dwExtraInfo: marker,
                    ..Default::default()
                },
            },
        };
        n += 1;
    }
    let begin = clock.now().0;
    let inserted = if n == 0 {
        0
    } else {
        // SAFETY: Process proof prevents competing Engine output; initialized release-only INPUT slice with valid size.
        unsafe { SendInput(&inputs[..n], size_of::<INPUT>() as i32) }
    };
    RecoveryReceipt {
        begin,
        end: clock.now().0,
        inserted,
        expected: n as u32,
    }
}
pub(crate) fn foreground_is(target: usize) -> bool {
    // SAFETY: GetForegroundWindow has no pointers; HWND is only compared, not dereferenced.
    unsafe { GetForegroundWindow() == HWND(target as *mut _) }
}

/// For desktop acceptance tests only; these foreign injections require test-hooks in the engine build.
#[cfg(feature = "test-hooks")]
pub fn inject_test_key(key: Key, down: bool) -> bool {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(u16::from(key.0)),
                dwFlags: if down {
                    KEYBD_EVENT_FLAGS(0)
                } else {
                    KEYEVENTF_KEYUP
                },
                ..Default::default()
            },
        },
    };
    // SAFETY: Initialized single INPUT, correct ABI size. This API is absent from production builds.
    unsafe { SendInput(&[input], size_of::<INPUT>() as i32) == 1 }
}
pub fn key_is_down(key: Key) -> bool {
    // SAFETY: Value-only VK lookup, no pointer or ownership transfer.
    unsafe { GetAsyncKeyState(i32::from(key.0)) < 0 }
}
