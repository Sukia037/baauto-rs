use crate::geometry::Rect;
use baauto_core::{SessionId, TickRate, Ticks};
use baauto_protocol::shm::*;
use std::{ffi::OsStr, mem::size_of, os::windows::ffi::OsStrExt, ptr::NonNull};
use windows::{
    Win32::{
        Foundation::*,
        Security::Cryptography::*,
        System::{Memory::*, Performance::*, Threading::*},
    },
    core::{Error, PCWSTR, Result},
};

pub(crate) fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}
pub(crate) fn failure(s: &str) -> Error {
    Error::new(E_FAIL, s)
}
pub struct OwnedHandle(pub(crate) HANDLE);
// SAFETY: Kernel handles are thread-safe, uniquely owned and remain open until the last containing owner drops.
unsafe impl Send for OwnedHandle {}
// SAFETY: Kernel object APIs synchronize access; borrowing never transfers or closes ownership.
unsafe impl Sync for OwnedHandle {}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: OwnedHandle owns a valid non-pseudo kernel handle, closed exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
pub struct Event {
    pub(crate) handle: OwnedHandle,
}
impl Event {
    pub fn new(manual: bool) -> Result<Self> {
        // SAFETY: No security pointer or name is passed; Windows returns a new owned event.
        Ok(Self {
            handle: OwnedHandle(unsafe { CreateEventW(None, manual, false, PCWSTR::null())? }),
        })
    }
    pub fn set(&self) -> Result<()> {
        // SAFETY: The event remains open for the duration of this shared borrow.
        unsafe { SetEvent(self.handle.0) }
    }
    pub fn signaled(&self) -> bool {
        // SAFETY: The valid event is polled without blocking. Stop events are manual-reset, never reset.
        unsafe { WaitForSingleObject(self.handle.0, 0) == WAIT_OBJECT_0 }
    }
    pub(crate) fn raw(&self) -> HANDLE {
        self.handle.0
    }
}
pub struct Timer {
    handle: OwnedHandle,
    pub degraded: bool,
}
impl Timer {
    pub fn new() -> Result<Self> {
        // SAFETY: No pointers are retained; timer is an owned kernel object.
        let high = unsafe {
            CreateWaitableTimerExW(
                None,
                PCWSTR::null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS.0,
            )
        };
        let (handle, degraded) = match high {
            Ok(h) => (h, false),
            Err(_) => {
                // SAFETY: Same arguments as above, without the optional high-resolution flag.
                (
                    unsafe { CreateWaitableTimerExW(None, PCWSTR::null(), 0, TIMER_ALL_ACCESS.0)? },
                    true,
                )
            }
        };
        Ok(Self {
            handle: OwnedHandle(handle),
            degraded,
        })
    }
    pub fn set_deadline(&self, deadline: Ticks, clock: Clock) -> Result<()> {
        let delta = deadline.0.saturating_sub(clock.now().0);
        let units = (u128::from(delta) * 10_000_000)
            .div_ceil(u128::from(clock.rate.0))
            .max(1)
            .min(i64::MAX as u128) as i64;
        let relative = -units;
        // SAFETY: Windows copies the stack due-time value; no APC callback, zero period.
        unsafe { SetWaitableTimer(self.handle.0, &relative, 0, None, None, false) }
    }
    pub(crate) fn raw(&self) -> HANDLE {
        self.handle.0
    }
}
#[derive(Clone, Copy)]
pub struct Clock {
    pub rate: TickRate,
}
impl Clock {
    pub fn new() -> Result<Self> {
        let mut freq = 0;
        // SAFETY: Pointer is valid initialized stack storage for one i64.
        unsafe {
            QueryPerformanceFrequency(&mut freq)?;
        }
        if freq <= 0 {
            return Err(failure("invalid QPC frequency"));
        }
        Ok(Self {
            rate: TickRate(freq as u64),
        })
    }
    pub fn now(self) -> Ticks {
        let mut qpc = 0;
        // SAFETY: Pointer is valid; QPC is supported on all supported Windows versions.
        unsafe {
            QueryPerformanceCounter(&mut qpc).expect("QPC failed");
        }
        Ticks(qpc as u64)
    }
}
pub fn random_name(prefix: &str) -> Result<String> {
    let mut random = [0; 16];
    // SAFETY: BCrypt writes only the provided slice; system-preferred RNG needs no algorithm handle.
    let status = unsafe { BCryptGenRandom(None, &mut random, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.0 < 0 {
        return Err(failure("system RNG failed"));
    }
    let suffix = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Ok(format!("Local\\{prefix}-{suffix}"))
}
pub fn random_marker() -> Result<usize> {
    // Native x64 mouse input can retain only the low 32 bits of extra info.
    // Use the same nonzero marker width for keyboard and mouse round trips.
    let mut random = [0; 4];
    // SAFETY: Valid output slice, system RNG without retained pointers.
    let status = unsafe { BCryptGenRandom(None, &mut random, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.0 < 0 {
        return Err(failure("system RNG failed"));
    }
    Ok((u32::from_le_bytes(random) | 1) as usize)
}
pub struct Mapping {
    handle: OwnedHandle,
    ptr: NonNull<SharedMemory>,
    pub name: String,
}
// SAFETY: The initialized mapping contains only atomics and immutable layout fields; its view remains live until Drop.
unsafe impl Send for Mapping {}
// SAFETY: All mutable shared fields use atomic access. Initialization occurs before this object is shared.
unsafe impl Sync for Mapping {}
impl Mapping {
    pub fn create(name: &str, session: SessionId, clock: Clock) -> Result<Self> {
        let encoded = wide(OsStr::new(name));
        // SAFETY: Pagefile-backed mapping, valid NUL-terminated name; no security pointer.
        let handle = OwnedHandle(unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                size_of::<SharedMemory>() as u32,
                PCWSTR(encoded.as_ptr()),
            )?
        });
        // SAFETY: Reading the calling thread's last error immediately after CreateFileMappingW.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Err(failure("mapping name already exists"));
        }
        // SAFETY: Valid mapping handle and full bounded view size; Windows supplies page-aligned storage.
        let view = unsafe {
            MapViewOfFile(
                handle.0,
                FILE_MAP_ALL_ACCESS,
                0,
                0,
                size_of::<SharedMemory>(),
            )
        };
        let ptr = NonNull::new(view.Value.cast::<SharedMemory>()).ok_or_else(Error::from_thread)?;
        // SAFETY: Newly created pagefile mapping is exclusively initialized before publication; no prior object is overwritten.
        unsafe {
            SharedMemory::initialize(ptr.as_ptr(), session, clock.rate.0);
        }
        Ok(Self {
            handle,
            ptr,
            name: name.into(),
        })
    }
    pub fn open(name: &str) -> Result<Self> {
        let encoded = wide(OsStr::new(name));
        // SAFETY: Valid NUL-terminated name; no inheritable handle.
        let handle = OwnedHandle(unsafe {
            OpenFileMappingW(FILE_MAP_ALL_ACCESS.0, false, PCWSTR(encoded.as_ptr()))?
        });
        // SAFETY: View length is the declared layout size and handle is live.
        let view = unsafe {
            MapViewOfFile(
                handle.0,
                FILE_MAP_ALL_ACCESS,
                0,
                0,
                size_of::<SharedMemory>(),
            )
        };
        let ptr = NonNull::new(view.Value.cast()).ok_or_else(Error::from_thread)?;
        let m = Self {
            handle,
            ptr,
            name: name.into(),
        };
        let h = &m.data().header;
        if h.magic != MAGIC
            || h.layout_version != LAYOUT_VERSION
            || h.size != size_of::<SharedMemory>() as u32
        {
            return Err(failure("shared-memory version mismatch"));
        }
        Ok(m)
    }
    pub fn data(&self) -> &SharedMemory {
        // SAFETY: View is live, aligned and completely initialized; safe users can only mutate atomics.
        unsafe { self.ptr.as_ref() }
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: View was mapped once and remains owned by self. Other borrows cannot outlive self.
        unsafe {
            let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr.as_ptr().cast(),
            });
        }
        let _ = &self.handle;
    }
}
pub(crate) fn wait(handles: &[HANDLE], timeout: u32) -> Result<usize> {
    // SAFETY: All handles are owned by live Event/Timer/Process objects in the caller; bounded slice <= MAXIMUM_WAIT_OBJECTS.
    let result = unsafe { WaitForMultipleObjects(handles, false, timeout) };
    if result == WAIT_FAILED {
        Err(Error::from_thread())
    } else if result == WAIT_TIMEOUT {
        Ok(usize::MAX)
    } else {
        Ok(result.0 as usize)
    }
}
pub fn virtual_desktop() -> Rect {
    use windows::Win32::UI::WindowsAndMessaging::*;
    // SAFETY: GetSystemMetrics takes no pointers and returns virtual-desktop physical pixels under PMv2 awareness.
    unsafe {
        Rect {
            left: GetSystemMetrics(SM_XVIRTUALSCREEN),
            top: GetSystemMetrics(SM_YVIRTUALSCREEN),
            width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}
/// The GUI build has no console of its own; console modes reuse the terminal that started them.
pub fn attach_parent_console() -> bool {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // SAFETY: Value-only call; failure just means there is no parent console to attach to.
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_ok()
}
pub fn set_dpi_awareness() -> Result<()> {
    use windows::Win32::UI::HiDpi::*;
    // SAFETY: Called before creating a window or hook; no pointers are involved.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
}
/// Executable names of running processes; used only for warnings, never on a hot path.
pub fn running_process_names() -> Result<Vec<String>> {
    use windows::Win32::System::Diagnostics::ToolHelp::*;
    // SAFETY: The snapshot handle is owned and closed by OwnedHandle; the entry is sized before each
    // call and szExeFile is a fixed in-struct buffer read only up to its NUL.
    unsafe {
        let snapshot = OwnedHandle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut names = Vec::new();
        let mut more = Process32FirstW(snapshot.0, &mut entry).is_ok();
        while more {
            let len = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            names.push(String::from_utf16_lossy(&entry.szExeFile[..len]));
            more = Process32NextW(snapshot.0, &mut entry).is_ok();
        }
        Ok(names)
    }
}
/// Modal error box for failures that happen before or instead of the GUI window.
pub fn error_dialog(title: &str, text: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let title = wide(OsStr::new(title));
    let text = wide(OsStr::new(text));
    // SAFETY: Both buffers are NUL-terminated and outlive the modal call; there is no owner window.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_stop_stays_signaled_and_sessions_get_distinct_events() {
        let first = Event::new(true).unwrap();
        let second = Event::new(true).unwrap();
        first.set().unwrap();
        assert!(first.signaled());
        assert!(first.signaled());
        assert!(!second.signaled());
    }
    #[test]
    fn auto_wake_is_consumed_without_reset() {
        let wake = Event::new(false).unwrap();
        wake.set().unwrap();
        assert!(wake.signaled());
        assert!(!wake.signaled());
    }
    #[test]
    fn waitable_timer_wakes_at_deadline() {
        let clock = Clock::new().unwrap();
        let timer = Timer::new().unwrap();
        let due = clock.now().after(clock.rate.0 / 100);
        timer.set_deadline(due, clock).unwrap();
        assert_eq!(wait(&[timer.raw()], 1000).unwrap(), 0);
        assert!(clock.now() >= due);
    }
    #[test]
    fn mapping_rejects_wrong_session_and_survives_creator_drop() {
        let clock = Clock::new().unwrap();
        let name = random_name("BAauto-test").unwrap();
        let creator = Mapping::create(&name, SessionId(42), clock).unwrap();
        assert!(!creator.data().header.valid(SessionId(41)));
        let reader = Mapping::open(&name).unwrap();
        creator
            .data()
            .ledger
            .before_send(&[baauto_core::OutEvent::LeftDown]);
        drop(creator);
        assert!(
            reader
                .data()
                .ledger
                .snapshot()
                .unwrap()
                .contains(baauto_core::OutKey::LeftButton)
        );
    }
}
