use super::*;
use crate::{
    geometry::Rect,
    hook::{Decision, HookState, RawEvent, RawKind},
};
use baauto_config::{CompiledConfig, Key};
use baauto_core::*;
use baauto_protocol::shm::{LogData, RingReader};
use rtrb::{Consumer, Producer, RingBuffer};
use std::{
    cell::Cell,
    ffi::OsStr,
    mem::size_of,
    path::Path,
    sync::{Arc, OnceLock, atomic::Ordering::SeqCst, mpsc},
    thread,
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{Console::*, LibraryLoader::*, Threading::*},
        UI::{Accessibility::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
    core::{PCWSTR, PWSTR, Result},
};

const WM_CONTROL: u32 = WM_APP + 1;
const WM_PROBE: u32 = WM_APP + 2;
thread_local! {static HOOK_RUNTIME:Cell<*mut HookRuntime>=const{Cell::new(std::ptr::null_mut())};}
#[derive(Clone, Copy)]
pub struct Target {
    pub hwnd: usize,
    pub pid: u32,
    pub monitor: Rect,
    pub window: Rect,
    pub geometry: GeometryVersion,
}
impl Target {
    pub fn warning(self) -> bool {
        !self.monitor.fullscreen_16x9(self.window)
    }
}
pub fn target_for_foreground(exe: &OsStr) -> Result<Option<Target>> {
    // SAFETY: Borrowed HWND is only used with Win32 inspection APIs; process handle has query-only access.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Ok(None);
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(p) => OwnedHandle(p),
            Err(_) => return Ok(None),
        };
        let mut buffer = [0u16; 32768];
        let mut len = buffer.len() as u32;
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )?;
        let path = std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..len as usize]));
        if path.file_name().is_none_or(|n| {
            !n.to_string_lossy()
                .eq_ignore_ascii_case(&exe.to_string_lossy())
        }) {
            return Ok(None);
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect)?;
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err(windows::core::Error::from_thread());
        }
        Ok(Some(Target {
            hwnd: hwnd.0 as usize,
            pid,
            monitor: Rect {
                left: info.rcMonitor.left,
                top: info.rcMonitor.top,
                width: info.rcMonitor.right - info.rcMonitor.left,
                height: info.rcMonitor.bottom - info.rcMonitor.top,
            },
            window: Rect {
                left: rect.left,
                top: rect.top,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
            },
            geometry: GeometryVersion(1),
        }))
    }
}
#[derive(Clone)]
pub struct RuntimeContext {
    pub cfg: Arc<CompiledConfig>,
    pub mapping: Arc<Mapping>,
    pub stop: Arc<Event>,
    pub wake: Arc<Event>,
    pub target: Target,
    pub marker: usize,
    pub session: SessionId,
    pub clock: Clock,
}
#[derive(Clone, Copy)]
pub struct QueuedInput {
    pub event: InputEvent,
    pub enqueue: Ticks,
    pub depth: u32,
}
struct HookRuntime {
    state: HookState,
    cfg: Arc<CompiledConfig>,
    producer: Producer<QueuedInput>,
    stop: Arc<Event>,
    wake: Arc<Event>,
    mapping: Arc<Mapping>,
    target: Target,
    clock: Clock,
}
impl HookRuntime {
    fn deliver(&mut self, d: Decision) {
        if d.physical {
            self.mapping
                .data()
                .progress
                .hook_physical_events
                .fetch_add(1, SeqCst);
        }
        if let Some(_reason) = d.stop {
            let _ = self.stop.set();
        }
        for event in d.events {
            let queued = QueuedInput {
                event,
                enqueue: self.clock.now(),
                depth: self
                    .cfg
                    .params
                    .queue_capacity
                    .saturating_sub(self.producer.slots() as u32),
            };
            if self.producer.push(queued).is_err() {
                self.state.invalidate();
                self.mapping
                    .data()
                    .progress
                    .overflow_count
                    .fetch_add(1, SeqCst);
                let _ = self.stop.set();
                break;
            }
            let _ = self.wake.set();
        }
    }
    fn publish_physical(&self) {
        for chunk in 0..4 {
            let bits = (0..64).fold(0u64, |bits, i| {
                bits | (u64::from(self.state.physical[chunk * 64 + i]) << i)
            });
            self.mapping.data().progress.physical_keys[chunk].store(bits, SeqCst);
        }
    }
    fn observe_async_held(&mut self) {
        let related = self.cfg.related_keys();
        for (i, needed) in related.into_iter().enumerate() {
            // SAFETY: Fixed VK range; value-only query used outside callbacks. Never clears Hook-tracked swallowed Down state.
            if needed && unsafe { GetAsyncKeyState(i as i32) } < 0 {
                self.state.physical[i] = true;
            }
        }
        self.publish_physical();
    }
    fn raw(&mut self, kind: RawKind, injected: bool, extra: usize, screen: POINT) -> bool {
        let qpc = self.clock.now();
        let ledger = &self.mapping.data().ledger;
        let d = self.state.decide(RawEvent {
            kind,
            injected,
            extra,
            cursor: self.target.monitor.norm(screen.x, screen.y),
            inside_window: self.target.window.inside(screen.x, screen.y),
            at: qpc,
            ledger_keys: std::array::from_fn(|i| ledger.held_keys[i].load(SeqCst)),
            ledger_mouse: ledger.held_mouse.load(SeqCst) & 1 != 0,
        });
        if d.physical {
            self.publish_physical();
        }
        let swallow = d.swallow;
        self.deliver(d);
        self.mapping
            .data()
            .progress
            .hook_last_call_qpc
            .store(qpc.0, SeqCst);
        swallow
    }
}
fn callback_failed() {
    HOOK_RUNTIME.with(|slot| {
        let ptr = slot.get();
        if !ptr.is_null() {
            // SAFETY: This helper is called on the owning Hook thread while its TLS pointer is registered.
            unsafe {
                (*ptr).state.invalidate();
                let _ = (*ptr).stop.set();
            }
        }
    });
}
unsafe extern "system" fn keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    let decision = std::panic::catch_unwind(|| {
        if code < 0 {
            return false;
        }
        HOOK_RUNTIME.with(|slot| {
            let ptr = slot.get();
            if ptr.is_null() {
                return false;
            }
            // SAFETY: Windows passes a live KBDLLHOOKSTRUCT for nonnegative hook codes. Runtime is exclusively accessed on its hook thread.
            unsafe {
                let event = &*(l.0 as *const KBDLLHOOKSTRUCT);
                if event.dwExtraInfo == (*ptr).state.marker
                    || event.flags.contains(LLKHF_INJECTED) && !cfg!(feature = "test-hooks")
                {
                    return false;
                }
                let down = matches!(w.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
                let mut cursor = POINT::default();
                if down && GetCursorPos(&mut cursor).is_err() {
                    (*ptr).state.invalidate();
                    let _ = (*ptr).stop.set();
                }
                (*ptr).raw(
                    RawKind::Key {
                        key: Key(event.vkCode as u8),
                        down: matches!(w.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN),
                    },
                    event.flags.contains(LLKHF_INJECTED),
                    event.dwExtraInfo,
                    cursor,
                )
            }
        })
    })
    .unwrap_or_else(|_| {
        callback_failed();
        false
    });
    if decision {
        LRESULT(1)
    } else {
        // SAFETY: Propagates the original parameters; optional hook handle is ignored for low-level hooks.
        unsafe { CallNextHookEx(None, code, w, l) }
    }
}
unsafe extern "system" fn mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    let decision = std::panic::catch_unwind(|| {
        if code < 0 {
            return false;
        }
        HOOK_RUNTIME.with(|slot| {
            let ptr = slot.get();
            if ptr.is_null() {
                return false;
            }
            // SAFETY: Windows supplies the live MSLLHOOKSTRUCT; all callbacks share the owning hook thread and do not pump messages recursively.
            unsafe {
                let event = &*(l.0 as *const MSLLHOOKSTRUCT);
                let msg = w.0 as u32;
                let (key, down) = match msg {
                    WM_LBUTTONDOWN => (1, true),
                    WM_LBUTTONUP => (1, false),
                    WM_RBUTTONDOWN => (2, true),
                    WM_RBUTTONUP => (2, false),
                    WM_MBUTTONDOWN => (4, true),
                    WM_MBUTTONUP => (4, false),
                    WM_XBUTTONDOWN | WM_XBUTTONUP => {
                        let key = if event.mouseData >> 16 == 1 { 5 } else { 6 };
                        (key, msg == WM_XBUTTONDOWN)
                    }
                    _ => return false,
                };
                (*ptr).raw(
                    RawKind::Key {
                        key: Key(key),
                        down,
                    },
                    event.flags & LLMHF_INJECTED != 0,
                    event.dwExtraInfo,
                    event.pt,
                )
            }
        })
    })
    .unwrap_or_else(|_| {
        callback_failed();
        false
    });
    if decision {
        LRESULT(1)
    } else {
        // SAFETY: Forwards original hook arguments, without modifying borrowed OS storage.
        unsafe { CallNextHookEx(None, code, w, l) }
    }
}
unsafe extern "system" fn focus_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    let result = std::panic::catch_unwind(|| {
        HOOK_RUNTIME.with(|slot| {
            let ptr = slot.get();
            if ptr.is_null() {
                return;
            }
            // SAFETY: Out-of-context WinEvent callbacks run on the registering hook thread. Pointer remains live until hooks are unregistered.
            unsafe {
                let rt = &mut *ptr;
                let focused = event == EVENT_SYSTEM_FOREGROUND && hwnd.0 as usize == rt.target.hwnd;
                if event == EVENT_OBJECT_LOCATIONCHANGE
                    && object == OBJID_WINDOW.0
                    && child == 0
                    && hwnd.0 as usize == rt.target.hwnd
                {
                    let mut window = RECT::default();
                    let mut monitor = MONITORINFO {
                        cbSize: size_of::<MONITORINFO>() as u32,
                        ..Default::default()
                    };
                    let geometry_changed = GetWindowRect(hwnd, &mut window).is_err()
                        || !GetMonitorInfoW(
                            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST),
                            &mut monitor,
                        )
                        .as_bool()
                        || window.left != rt.target.window.left
                        || window.top != rt.target.window.top
                        || window.right - window.left != rt.target.window.width
                        || window.bottom - window.top != rt.target.window.height
                        || monitor.rcMonitor.left != rt.target.monitor.left
                        || monitor.rcMonitor.top != rt.target.monitor.top
                        || monitor.rcMonitor.right - monitor.rcMonitor.left
                            != rt.target.monitor.width
                        || monitor.rcMonitor.bottom - monitor.rcMonitor.top
                            != rt.target.monitor.height;
                    if geometry_changed {
                        rt.state.geometry.0 = rt.state.geometry.0.saturating_add(1);
                        rt.state.invalidate();
                        let _ = rt.stop.set();
                    }
                } else if event == EVENT_SYSTEM_FOREGROUND {
                    let d = rt.state.focus(focused, rt.clock.now());
                    rt.deliver(d);
                }
            }
        })
    });
    if result.is_err() {
        callback_failed();
    }
}
enum HookCommand {
    Arm {
        session: SessionId,
        stop: Arc<Event>,
        reply: mpsc::Sender<bool>,
    },
    Disarm,
    Released(mpsc::Sender<bool>),
    Shutdown,
}
pub struct HookThread {
    thread_id: u32,
    commands: mpsc::Sender<HookCommand>,
    join: Option<thread::JoinHandle<Result<()>>>,
}
impl HookThread {
    pub fn spawn(context: RuntimeContext, producer: Producer<QueuedInput>) -> Result<Self> {
        let RuntimeContext {
            cfg,
            marker,
            stop,
            wake,
            mapping,
            target,
            clock,
            ..
        } = context;
        let (commands, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let join = thread::spawn(move || {
            let mut rt = Box::new(HookRuntime {
                state: HookState::new(&cfg, marker),
                cfg,
                producer,
                stop,
                wake,
                mapping,
                target,
                clock,
            });
            // SAFETY: OS keyboard snapshot is obtained before installing hooks; subsequent physical changes are tracked by callbacks.
            rt.state.seed_physical(std::array::from_fn(
                |i| unsafe { GetAsyncKeyState(i as i32) } < 0,
            ));
            rt.publish_physical();
            rt.state.focused = foreground_is(target.hwnd);
            rt.state.geometry = target.geometry;
            // SAFETY: Thread priority affects only this dedicated thread. PeekMessage initializes its message queue before publishing thread ID.
            unsafe {
                SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL)?;
                if GetThreadPriority(GetCurrentThread()) != THREAD_PRIORITY_TIME_CRITICAL.0 {
                    return Err(failure("Hook thread priority was not applied"));
                }
                let mut msg = MSG::default();
                let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            }
            HOOK_RUNTIME.with(|s| s.set(&mut *rt));
            // SAFETY: Current module stays loaded throughout process lifetime; static callbacks meet extern system ABI and catch panics.
            let (kh, mh, fh, gh) = unsafe {
                let module = GetModuleHandleW(PCWSTR::null())?;
                let kh = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(keyboard),
                    Some(HINSTANCE(module.0)),
                    0,
                )?;
                let mh =
                    match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), Some(HINSTANCE(module.0)), 0)
                    {
                        Ok(h) => h,
                        Err(e) => {
                            let _ = UnhookWindowsHookEx(kh);
                            return Err(e);
                        }
                    };
                let fh = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(focus_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                );
                let gh = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(focus_event),
                    target.pid,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                );
                if fh.0.is_null() || gh.0.is_null() {
                    let error = windows::core::Error::from_thread();
                    let _ = UnhookWindowsHookEx(kh);
                    let _ = UnhookWindowsHookEx(mh);
                    if !fh.0.is_null() {
                        let _ = UnhookWinEvent(fh);
                    }
                    if !gh.0.is_null() {
                        let _ = UnhookWinEvent(gh);
                    }
                    HOOK_RUNTIME.with(|s| s.set(std::ptr::null_mut()));
                    return Err(error);
                }
                (kh, mh, fh, gh)
            };
            // SAFETY: Hooks are installed but the session is not armed and no press can have been intercepted. A fresh snapshot closes the pre-install observation gap.
            rt.state.seed_physical(std::array::from_fn(
                |i| unsafe { GetAsyncKeyState(i as i32) } < 0,
            ));
            rt.publish_physical();
            // SAFETY: GetCurrentThreadId is a value lookup with no pointer arguments.
            let id = unsafe { GetCurrentThreadId() };
            let _ = ready_tx.send(id);
            let result = (|| {
                let mut msg = MSG::default();
                loop {
                    // SAFETY: Valid initialized MSG storage; no borrowed pointer escapes the call. This wait is on the Hook message loop, outside callbacks.
                    let status = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
                    if status == -1 {
                        return Err(windows::core::Error::from_thread());
                    }
                    if status == 0 {
                        break;
                    }
                    if msg.message == WM_PROBE {
                        rt.mapping.data().progress.hook_loop.fetch_add(1, SeqCst);
                    }
                    if msg.message == WM_CONTROL {
                        while let Ok(cmd) = rx.try_recv() {
                            match cmd {
                                HookCommand::Arm {
                                    session,
                                    stop,
                                    reply,
                                } => {
                                    rt.stop = stop;
                                    rt.state.focused = foreground_is(target.hwnd);
                                    rt.observe_async_held();
                                    let accepted = rt.state.arm(session, &rt.cfg);
                                    let _ = reply.send(accepted);
                                }
                                HookCommand::Disarm => rt.state.invalidate(),
                                HookCommand::Released(reply) => {
                                    let _ = reply.send(rt.state.keys_released(&rt.cfg));
                                }
                                HookCommand::Shutdown => return Ok(()),
                            }
                        }
                    }
                    // SAFETY: MSG was filled by GetMessageW; dispatch may invoke only registered callbacks on this same thread.
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                Ok(())
            })();
            // SAFETY: Hooks are owned by this thread and runtime remains live until unregister completes. No callback can outlive the message loop.
            unsafe {
                let _ = UnhookWindowsHookEx(kh);
                let _ = UnhookWindowsHookEx(mh);
                let _ = UnhookWinEvent(fh);
                let _ = UnhookWinEvent(gh);
            }
            HOOK_RUNTIME.with(|s| s.set(std::ptr::null_mut()));
            result
        });
        let thread_id = ready_rx
            .recv()
            .map_err(|_| failure("hook initialization failed"))?;
        Ok(Self {
            thread_id,
            commands,
            join: Some(join),
        })
    }
    fn command(&self, command: HookCommand) -> Result<()> {
        self.commands
            .send(command)
            .map_err(|_| failure("hook thread ended"))?;
        // SAFETY: Published thread ID has an initialized message queue; message has no borrowed pointer payload.
        unsafe { PostThreadMessageW(self.thread_id, WM_CONTROL, WPARAM(0), LPARAM(0)) }
    }
    pub fn arm(&self, session: SessionId, stop: Arc<Event>) -> Result<bool> {
        let (tx, rx) = mpsc::channel();
        self.command(HookCommand::Arm {
            session,
            stop,
            reply: tx,
        })?;
        rx.recv().map_err(|_| failure("hook arm failed"))
    }
    pub fn disarm(&self) -> Result<()> {
        self.command(HookCommand::Disarm)
    }
    pub fn keys_released(&self) -> Result<bool> {
        let (tx, rx) = mpsc::channel();
        self.command(HookCommand::Released(tx))?;
        rx.recv().map_err(|_| failure("hook query failed"))
    }
    pub fn probe(&self) -> Result<()> {
        // SAFETY: No pointer-bearing message is posted; only the Hook loop updates its progress counter.
        unsafe { PostThreadMessageW(self.thread_id, WM_PROBE, WPARAM(0), LPARAM(0)) }
    }
    pub fn shutdown(mut self) -> Result<()> {
        self.command(HookCommand::Shutdown)?;
        self.join
            .take()
            .unwrap()
            .join()
            .map_err(|_| failure("hook thread panicked"))?
    }
}
#[derive(Clone, Copy, Default)]
struct Trace {
    generation: u32,
    hook: u64,
    enqueue: u64,
    receive: u64,
    seq: u64,
    depth: u32,
}
struct DoneGuard(Arc<Event>);
impl Drop for DoneGuard {
    fn drop(&mut self) {
        let _ = self.0.set();
    }
}
pub struct EngineThread {
    pub done: Arc<Event>,
    join: Option<thread::JoinHandle<Result<Consumer<QueuedInput>>>>,
}
impl EngineThread {
    pub fn spawn(context: RuntimeContext, mut input: Consumer<QueuedInput>) -> Result<Self> {
        let RuntimeContext {
            cfg,
            mapping,
            stop,
            wake,
            target,
            marker,
            session,
            clock,
        } = context;
        let done = Arc::new(Event::new(true)?);
        let finished = done.clone();
        let join = thread::spawn(move || {
            let _done = DoneGuard(finished);
            let mut engine = Engine::new(cfg.clone(), clock.rate);
            let mut driver = Driver::new(
                marker,
                stop.clone(),
                mapping.clone(),
                target.hwnd,
                target.monitor,
                clock,
            );
            let mut traces = [Trace::default(); 256];
            let mut stop_logged = false;
            let timer = Timer::new()?;
            if timer.degraded {
                mapping.data().progress.degraded_flags.fetch_or(1, SeqCst);
            }
            engine
                .arm(session, clock.now())
                .map_err(|_| failure("engine arm refused"))?;
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
                    loop {
                        let now = clock.now();
                        if stop.signaled() {
                            if !stop_logged {
                                let mut fields = [0; 16];
                                fields[0] = 3;
                                fields[1] = u64::from(session.0);
                                fields[6] = now.0;
                                mapping.data().ring.push(LogData(fields));
                                stop_logged = true;
                            }
                            engine.request_stop(StopReason::User, now);
                        }
                        for _ in 0..cfg.params.max_events_per_round {
                            if stop.signaled() {
                                engine.request_stop(StopReason::User, clock.now());
                                break;
                            }
                            let Ok(q) = input.pop() else {
                                break;
                            };
                            let received = clock.now();
                            engine.on_input(q.event, received);
                            if let InputKind::TriggerDown { press, .. }
                            | InputKind::TriggerRepeat { press, .. } = q.event.kind
                            {
                                for id in engine.actions_for_press(press) {
                                    let t = &mut traces[id.slot as usize];
                                    if t.generation != id.generation {
                                        *t = Trace {
                                            generation: id.generation,
                                            hook: q.event.at.0,
                                            enqueue: q.enqueue.0,
                                            receive: received.0,
                                            seq: q.event.seq,
                                            depth: q.depth,
                                        };
                                    }
                                }
                            }
                            let mut fields = [0; 16];
                            fields[0] = 1;
                            fields[1] = u64::from(session.0);
                            fields[4] = q.event.at.0;
                            fields[5] = q.enqueue.0;
                            fields[6] = received.0;
                            fields[12] = u64::from(q.depth);
                            fields[13] = u64::from(q.event.config.0);
                            fields[14] = u64::from(q.event.geometry.0);
                            fields[15] = q.event.seq;
                            mapping.data().ring.push(LogData(fields));
                        }
                        for _ in 0..cfg.params.max_batches_per_round {
                            if stop.signaled() {
                                engine.request_stop(StopReason::User, clock.now());
                            }
                            let Some(b) = engine.next_batch(clock.now()) else {
                                break;
                            };
                            let result = driver.submit(&b);
                            let t = b
                                .action
                                .map_or(Trace::default(), |a| traces[a.slot as usize]);
                            let mut fields = [0; 16];
                            fields[0] = 2;
                            fields[1] = u64::from(session.0);
                            fields[2] = b
                                .action
                                .map_or(0, |a| (u64::from(a.generation) << 16) | u64::from(a.slot));
                            fields[3] = u64::from(b.stage);
                            fields[4] = t.hook;
                            fields[5] = t.enqueue;
                            fields[6] = t.receive;
                            fields[7] = b.deadline.0;
                            fields[12] = u64::from(t.depth);
                            fields[13] = u64::from(cfg.version.0);
                            fields[14] = u64::from(target.geometry.0);
                            fields[15] = t.seq;
                            match result {
                                SubmitOutcome::Ok { begin, end } => {
                                    fields[8] = begin.0;
                                    fields[9] = end.0;
                                    fields[10] = b.events.len() as u64;
                                }
                                SubmitOutcome::Short { inserted, end } => {
                                    fields[9] = end.0;
                                    fields[10] = u64::from(inserted);
                                }
                                SubmitOutcome::NotSubmitted => {
                                    engine.request_stop(StopReason::FocusLost, clock.now());
                                    let _ = stop.set();
                                }
                            }
                            fields[11] = b.events.len() as u64;
                            mapping.data().ring.push(LogData(fields));
                            engine.commit(b.id, result, clock.now());
                        }
                        let progress = engine.progress();
                        mapping
                            .data()
                            .progress
                            .scheduler
                            .store(progress.scheduler_progress, SeqCst);
                        mapping
                            .data()
                            .progress
                            .action
                            .store(progress.action_progress, SeqCst);
                        mapping
                            .data()
                            .header
                            .engine_state
                            .store(state_code(engine.state()), SeqCst);
                        if matches!(engine.state(), EngineState::Stopped(_)) {
                            let mut fields = [0; 16];
                            fields[0] = 4;
                            fields[1] = u64::from(session.0);
                            fields[9] = clock.now().0;
                            mapping.data().ring.push(LogData(fields));
                            return Ok(());
                        }
                        if matches!(engine.state(), EngineState::Faulted(..)) {
                            let _ = stop.set();
                            return Err(failure("input fault: ledger retained"));
                        }
                        let deadline = engine.next_wake().unwrap();
                        if !input.is_empty() || deadline <= clock.now() {
                            continue;
                        }
                        timer.set_deadline(deadline, clock)?;
                        if !input.is_empty() {
                            continue;
                        }
                        wait(&[stop.raw(), wake.raw(), timer.raw()], INFINITE)?;
                    }
                }));
            let outcome = match result {
                Ok(r) => r,
                Err(_) => Err(failure("engine panic")),
            };
            if outcome.is_err() {
                let _ = stop.set();
                mapping.data().header.engine_state.store(5, SeqCst);
                driver.cleanup_once(session);
            }
            outcome.map(|_| input)
        });
        Ok(Self {
            done,
            join: Some(join),
        })
    }
    pub fn join(mut self) -> Result<Consumer<QueuedInput>> {
        self.join
            .take()
            .unwrap()
            .join()
            .map_err(|_| failure("engine thread panicked"))?
    }
}
pub fn state_code(s: EngineState) -> u32 {
    match s {
        EngineState::Disarmed => 0,
        EngineState::Armed(_) => 1,
        EngineState::Running(_) => 2,
        EngineState::Stopping(..) => 3,
        EngineState::Stopped(_) => 4,
        EngineState::Faulted(..) => 5,
    }
}
static CONSOLE_STOP: OnceLock<Arc<Event>> = OnceLock::new();
unsafe extern "system" fn console(kind: u32) -> windows::core::BOOL {
    if matches!(
        kind,
        CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT | CTRL_SHUTDOWN_EVENT
    ) && let Some(e) = CONSOLE_STOP.get()
    {
        let _ = e.set();
        return true.into();
    }
    false.into()
}
pub fn console_stop() -> Result<Arc<Event>> {
    if let Some(e) = CONSOLE_STOP.get() {
        return Ok(e.clone());
    }
    let event = Arc::new(Event::new(true)?);
    let _ = CONSOLE_STOP.set(event.clone());
    // SAFETY: Static handler retains a process-lifetime Event through OnceLock, so no callback can see a closed handle.
    unsafe {
        SetConsoleCtrlHandler(Some(console), true)?;
    }
    Ok(event)
}
pub fn dump_ring(mapping: &Mapping, path: &Path) -> std::io::Result<u64> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    writeln!(
        file,
        "seq,kind,session,action,stage,hook_enter,enqueue,engine_receive,deadline,send_begin,send_end,inserted,expected,queue_depth,config_version,geometry_version,source_seq"
    )?;
    let mut reader = RingReader::default();
    while let Some((seq, data)) = reader.read(&mapping.data().ring) {
        write!(file, "{seq}")?;
        for value in data.0 {
            write!(file, ",{value}")?;
        }
        writeln!(file)?;
    }
    Ok(reader.lost)
}
pub fn run_standalone(
    cfg: Arc<CompiledConfig>,
    target_exe: &OsStr,
    max_secs: Option<u64>,
    dump: Option<&Path>,
) -> Result<()> {
    let _ = set_dpi_awareness();
    let clock = Clock::new()?;
    let console = console_stop()?;
    let discovery_timer = Timer::new()?;
    eprintln!("請將目標視窗切到前景；按 Ctrl+C 可取消。");
    let target = loop {
        if console.signaled() {
            return Ok(());
        }
        if let Some(target) = target_for_foreground(target_exe)? {
            break target;
        }
        discovery_timer.set_deadline(
            clock
                .now()
                .after(u64::from(cfg.params.health_period_us) * clock.rate.0 / 1_000_000),
            clock,
        )?;
        wait(&[console.raw(), discovery_timer.raw()], INFINITE)?;
    };
    if target.warning() {
        eprintln!("遊戲不是全螢幕 16:9，點位和牌鍵會不準");
    }
    let name = random_name("BAauto-rs")?;
    let mapping = Arc::new(Mapping::create(&name, SessionId(1), clock)?);
    mapping
        .data()
        .header
        .config_version
        .store(cfg.version.0, SeqCst);
    let stop = Arc::new(Event::new(true)?);
    let wake = Arc::new(Event::new(false)?);
    let marker = random_marker()?;
    let (producer, consumer) = RingBuffer::new(cfg.params.queue_capacity as usize);
    let context = RuntimeContext {
        cfg: cfg.clone(),
        mapping: mapping.clone(),
        stop: stop.clone(),
        wake,
        target,
        marker,
        session: SessionId(1),
        clock,
    };
    let hook = HookThread::spawn(context.clone(), producer)?;
    if !hook.arm(SessionId(1), stop.clone())? {
        hook.shutdown()?;
        return Err(failure("release relevant keys before arming"));
    }
    let engine = match EngineThread::spawn(context, consumer) {
        Ok(engine) => engine,
        Err(error) => {
            let _ = stop.set();
            let _ = hook.shutdown();
            return Err(error);
        }
    };
    let control_result = (|| -> Result<()> {
        let timer = Timer::new()?;
        let started = clock.now();
        let mut stopping = false;
        loop {
            if engine.done.signaled() {
                break;
            }
            if !stopping
                && (console.signaled()
                    || max_secs.is_some_and(|secs| {
                        clock.now().0.saturating_sub(started.0) >= secs.saturating_mul(clock.rate.0)
                    }))
            {
                stop.set()?;
                stopping = true;
            }
            if !stopping {
                hook.probe()?;
            }
            timer.set_deadline(
                clock
                    .now()
                    .after(u64::from(cfg.params.health_period_us) * clock.rate.0 / 1_000_000),
                clock,
            )?;
            if stopping {
                wait(&[engine.done.raw(), timer.raw()], INFINITE)?;
            } else {
                wait(&[engine.done.raw(), console.raw(), timer.raw()], INFINITE)?;
            }
        }
        Ok(())
    })();
    if control_result.is_err() {
        let _ = stop.set();
    }
    let outcome = engine.join();
    let _ = hook.disarm();
    hook.shutdown()?;
    if let Some(path) = dump {
        let lost = dump_ring(&mapping, path).map_err(|_| failure("ring dump failed"))?;
        println!(
            "ring: {name}; records: {}; lost: {lost}",
            mapping.data().ring.write_seq.load(SeqCst)
        );
    }
    control_result.and(outcome.map(|_| ()))
}
