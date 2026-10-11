use super::*;
use std::{
    ffi::{OsStr, OsString},
    mem::size_of,
};
use windows::{
    Win32::{Foundation::*, System::Threading::*},
    core::{PCWSTR, PWSTR, Result},
};
/// An owned process identity. A termination request never constitutes exit proof.
pub struct Process {
    pub(crate) handle: OwnedHandle,
    pub pid: u32,
}
fn quote(arg: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let mut out = vec![34];
    let mut slashes = 0;
    for unit in arg.encode_wide() {
        if unit == 92 {
            slashes += 1;
            continue;
        }
        if unit == 34 {
            out.extend(std::iter::repeat_n(92, slashes * 2 + 1));
            out.push(34);
        } else {
            out.extend(std::iter::repeat_n(92, slashes));
            out.push(unit);
        }
        slashes = 0;
    }
    out.extend(std::iter::repeat_n(92, slashes * 2));
    out.push(34);
    out
}
impl Process {
    pub fn spawn(exe: &OsStr, args: &[OsString]) -> Result<Self> {
        let app = wide(exe);
        let mut command = quote(exe);
        for arg in args {
            command.push(32);
            command.extend(quote(arg));
        }
        command.push(0);
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: Both strings are NUL terminated; command is writable through the call; no handles inherited and all output structs are valid.
        unsafe {
            CreateProcessW(
                PCWSTR(app.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                false,
                // Engine and receiver children never get a console window of their own.
                CREATE_NO_WINDOW,
                None,
                PCWSTR::null(),
                &startup,
                &mut info,
            )?;
        }
        drop(OwnedHandle(info.hThread));
        Ok(Self {
            handle: OwnedHandle(info.hProcess),
            pid: info.dwProcessId,
        })
    }
    pub fn parent(pid: u32) -> Result<Self> {
        // SAFETY: Opens the identified parent with synchronize-only access and retains the returned owned handle.
        Ok(Self {
            handle: OwnedHandle(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid)? }),
            pid,
        })
    }
    pub fn exited(&self) -> bool {
        // SAFETY: Live process handle; zero timeout is a nonblocking identity-specific exit check.
        unsafe { WaitForSingleObject(self.handle.0, 0) == WAIT_OBJECT_0 }
    }
    pub fn exit_proof(&self) -> Option<ExitedProcessProof> {
        self.exited().then_some(ExitedProcessProof { _private: () })
    }
    pub fn terminate(&self) -> Result<()> {
        // SAFETY: Owned child process handle created with terminate rights. This call only requests termination.
        unsafe { TerminateProcess(self.handle.0, 1) }
    }
    pub fn wait_exit(&self) -> Result<ExitedProcessProof> {
        wait(&[self.handle.0], INFINITE)?;
        self.exit_proof()
            .ok_or_else(|| failure("process did not exit"))
    }
    pub fn duplicate_event(&self, event: &Event) -> Result<u64> {
        let mut copied = HANDLE::default();
        // SAFETY: Source Event and target child handles are live. Child receives unique ownership of a noninheritable duplicate.
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                event.raw(),
                self.handle.0,
                &mut copied,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )?;
        }
        Ok(copied.0 as usize as u64)
    }
    pub fn cpu_100ns(&self) -> Result<u64> {
        let (mut creation, mut exit, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: The owned process handle remains live through this value-only query; stack FILETIME outputs are valid.
        unsafe {
            GetProcessTimes(
                self.handle.0,
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )?;
        }
        let value =
            |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        Ok(value(kernel).saturating_add(value(user)))
    }
    pub(crate) fn raw(&self) -> HANDLE {
        self.handle.0
    }
}
impl Event {
    pub(crate) fn take_transferred(handle: u64) -> Result<Self> {
        if handle == 0 || handle == u64::MAX {
            return Err(failure("invalid transferred event"));
        }
        let handle = HANDLE(handle as usize as *mut _);
        let mut flags = 0;
        // SAFETY: IPC peer is the owning parent, and this handle was duplicated into this process exactly once. Validation does not dereference its integer value.
        unsafe {
            GetHandleInformation(handle, &mut flags)?;
        }
        Ok(Self {
            handle: OwnedHandle(handle),
        })
    }
}
