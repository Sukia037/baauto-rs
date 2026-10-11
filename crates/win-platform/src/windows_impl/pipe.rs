//! Overlapped I/O lives on background workers. Each operation has its own Event and OVERLAPPED.
use super::*;
use baauto_protocol::ipc::{MAX_FRAME, decode_frame, encode_frame};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    ffi::OsStr,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};
use windows::{
    Win32::{
        Foundation::*,
        Storage::FileSystem::*,
        System::{IO::*, Pipes::*, Threading::INFINITE},
    },
    core::{PCWSTR, Result},
};
pub struct Pipe {
    handle: OwnedHandle,
}
impl Pipe {
    pub fn server(name: &str) -> Result<Self> {
        let encoded = wide(OsStr::new(name));
        // SAFETY: Valid pipe name; default per-user DACL, noninherited handle, one local instance, byte stream framing.
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(encoded.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65536,
                65536,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(windows::core::Error::from_thread());
        }
        Ok(Self {
            handle: OwnedHandle(handle),
        })
    }
    pub fn client(name: &str) -> Result<Self> {
        let encoded = wide(OsStr::new(name));
        // SAFETY: Server exists before child launch; no inheritable handle or security pointer. Each subsequent operation uses OVERLAPPED.
        Ok(Self {
            handle: OwnedHandle(unsafe {
                CreateFileW(
                    PCWSTR(encoded.as_ptr()),
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_MODE(0),
                    None,
                    OPEN_EXISTING,
                    FILE_FLAG_OVERLAPPED,
                    None,
                )?
            }),
        })
    }
    fn finish(
        &self,
        initial: Result<()>,
        overlap: &mut OVERLAPPED,
        abort: &Event,
        peer: &Process,
        timeout: u32,
    ) -> Result<u32> {
        if let Err(error) = initial
            && error.code() != windows::core::HRESULT::from_win32(ERROR_IO_PENDING.0)
        {
            return Err(error);
        }
        let waited = wait(&[overlap.hEvent, abort.raw(), peer.raw()], timeout);
        if !matches!(waited, Ok(0)) {
            // SAFETY: OVERLAPPED and operation buffer remain live until cancellation completion is drained below. CancelIoEx alone is not completion proof.
            unsafe {
                let _ = CancelIoEx(self.handle.0, Some(overlap));
                let mut bytes = 0;
                let _ = GetOverlappedResult(self.handle.0, overlap, &mut bytes, true);
            }
            return Err(failure("pipe operation cancelled or timed out"));
        }
        let mut bytes = 0;
        // SAFETY: Completion Event signaled and OVERLAPPED remains at its original address. No buffer is dropped before this result check.
        unsafe {
            GetOverlappedResult(self.handle.0, overlap, &mut bytes, false)?;
        }
        Ok(bytes)
    }
    pub fn connect(&self, abort: &Event, peer: &Process, timeout: u32) -> Result<()> {
        let event = Event::new(true)?;
        let mut overlap = OVERLAPPED {
            hEvent: event.raw(),
            ..Default::default()
        };
        // SAFETY: Live owned pipe; stack OVERLAPPED remains live through completion/cancellation.
        let started = unsafe { ConnectNamedPipe(self.handle.0, Some(&mut overlap)) };
        if started
            .as_ref()
            .is_err_and(|e| e.code() == windows::core::HRESULT::from_win32(ERROR_PIPE_CONNECTED.0))
        {
            return Ok(());
        }
        if started.is_ok() {
            return Ok(());
        }
        self.finish(started, &mut overlap, abort, peer, timeout)
            .map(|_| ())
    }
    fn read_exact(&self, buffer: &mut [u8], abort: &Event, peer: &Process) -> Result<()> {
        let mut done = 0;
        while done < buffer.len() {
            let event = Event::new(true)?;
            let mut overlap = OVERLAPPED {
                hEvent: event.raw(),
                ..Default::default()
            };
            let mut immediate = 0;
            // SAFETY: Mutable buffer is exclusively borrowed and remains alive until finish drains the operation. Independent read OVERLAPPED is not reused by writer.
            let initial = unsafe {
                ReadFile(
                    self.handle.0,
                    Some(&mut buffer[done..]),
                    Some(&mut immediate),
                    Some(&mut overlap),
                )
            };
            let n = if initial.is_ok() {
                immediate
            } else {
                self.finish(initial, &mut overlap, abort, peer, INFINITE)?
            } as usize;
            if n == 0 {
                return Err(failure("pipe disconnected"));
            }
            done += n;
        }
        Ok(())
    }
    pub fn read<T: DeserializeOwned>(&self, abort: &Event, peer: &Process) -> Result<T> {
        let mut prefix = [0; 4];
        self.read_exact(&mut prefix, abort, peer)?;
        let length = u32::from_le_bytes(prefix) as usize;
        if length > MAX_FRAME {
            return Err(failure("oversized pipe frame"));
        }
        let mut bytes = vec![0; length + 4];
        bytes[..4].copy_from_slice(&prefix);
        self.read_exact(&mut bytes[4..], abort, peer)?;
        decode_frame(&bytes).map_err(|_| failure("invalid pipe frame"))
    }
    pub fn write<T: Serialize>(&self, value: &T, abort: &Event, peer: &Process) -> Result<()> {
        let bytes = encode_frame(value).map_err(|_| failure("pipe encode failed"))?;
        let mut done = 0;
        while done < bytes.len() {
            let event = Event::new(true)?;
            let mut overlap = OVERLAPPED {
                hEvent: event.raw(),
                ..Default::default()
            };
            let mut immediate = 0;
            // SAFETY: Immutable frame bytes remain live until operation completion/cancellation. Writer alone owns write ordering.
            let initial = unsafe {
                WriteFile(
                    self.handle.0,
                    Some(&bytes[done..]),
                    Some(&mut immediate),
                    Some(&mut overlap),
                )
            };
            let n = if initial.is_ok() {
                immediate
            } else {
                self.finish(initial, &mut overlap, abort, peer, 2000)?
            } as usize;
            if n == 0 {
                return Err(failure("pipe write returned zero"));
            }
            done += n;
        }
        Ok(())
    }
}
pub struct PipePump<Incoming, Outgoing> {
    pub incoming: mpsc::Receiver<Incoming>,
    outgoing: Option<mpsc::SyncSender<Outgoing>>,
    pub closed: Arc<Event>,
    abort: Arc<Event>,
    reader: Option<thread::JoinHandle<()>>,
    writer: Option<thread::JoinHandle<()>>,
}
impl<I: DeserializeOwned + Send + 'static, O: Serialize + Send + 'static> PipePump<I, O> {
    pub fn start(pipe: Arc<Pipe>, peer: Arc<Process>) -> Result<Self> {
        let abort = Arc::new(Event::new(true)?);
        let closed = Arc::new(Event::new(true)?);
        let (tx_in, incoming) = mpsc::sync_channel(32);
        let (tx_out, rx_out) = mpsc::sync_channel(32);
        let reader = {
            let (pipe, peer, abort, closed) =
                (pipe.clone(), peer.clone(), abort.clone(), closed.clone());
            thread::spawn(move || {
                while let Ok(message) = pipe.read::<I>(&abort, &peer) {
                    if tx_in.try_send(message).is_err() {
                        break;
                    }
                }
                let _ = closed.set();
                let _ = abort.set();
            })
        };
        let writer = {
            let (abort, closed) = (abort.clone(), closed.clone());
            thread::spawn(move || {
                while !abort.signaled() {
                    match rx_out.recv_timeout(Duration::from_millis(50)) {
                        Ok(message) => {
                            if pipe.write(&message, &abort, &peer).is_err() {
                                break;
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    }
                }
                let _ = closed.set();
                let _ = abort.set();
            })
        };
        Ok(Self {
            incoming,
            outgoing: Some(tx_out),
            closed,
            abort,
            reader: Some(reader),
            writer: Some(writer),
        })
    }
    pub fn send(&self, message: O) -> Result<()> {
        self.outgoing
            .as_ref()
            .ok_or_else(|| failure("pipe stopped"))?
            .try_send(message)
            .map_err(|_| failure("pipe command queue full or closed"))
    }
    pub fn disconnect(&self) {
        let _ = self.abort.set();
    }
}
impl<I, O> Drop for PipePump<I, O> {
    fn drop(&mut self) {
        let _ = self.abort.set();
        self.outgoing.take();
        if let Some(worker) = self.reader.take() {
            let _ = worker.join();
        }
        if let Some(worker) = self.writer.take() {
            let _ = worker.join();
        }
    }
}
