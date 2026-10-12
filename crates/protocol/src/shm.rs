use baauto_core::{Key, OutEvent, OutKey, SessionId};
use std::mem::{align_of, size_of};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::SeqCst};

pub const MAGIC: u64 = 0x4241_4155_544f_5253;
pub const LAYOUT_VERSION: u32 = 4;
pub const RING_LEN: usize = 8192;
pub const LOG_FIELDS: usize = 16;
#[repr(C)]
pub struct Header {
    pub magic: u64,
    pub layout_version: u32,
    pub size: u32,
    pub session: AtomicU32,
    pub config_version: AtomicU32,
    pub engine_state: AtomicU32,
    pub reserved: u32,
    pub qpc_frequency: u64,
}
impl Header {
    pub fn valid(&self, session: SessionId) -> bool {
        self.magic == MAGIC
            && self.layout_version == LAYOUT_VERSION
            && self.size == size_of::<SharedMemory>() as u32
            && self.session.load(SeqCst) == session.0
    }
}
#[repr(C)]
pub struct Progress {
    pub scheduler: AtomicU64,
    pub action: AtomicU64,
    pub hook_loop: AtomicU64,
    pub hook_last_call_qpc: AtomicU64,
    pub hook_physical_events: AtomicU64,
    pub hook_bound_events: AtomicU64,
    pub overflow_count: AtomicU64,
    pub degraded_flags: AtomicU32,
    pub stop_reason: AtomicU32,
    pub stop_request_qpc: AtomicU64,
    /// Held in Windows by the user, not swallowed as a trigger. Read by the Driver before a Down.
    pub physical_keys: [AtomicU64; 4],
    /// Virtual key (1 = left button) of the last physical conflict, for the log; 0 = none.
    pub conflict_key: AtomicU64,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            scheduler: AtomicU64::new(0),
            action: AtomicU64::new(0),
            hook_loop: AtomicU64::new(0),
            hook_last_call_qpc: AtomicU64::new(0),
            hook_physical_events: AtomicU64::new(0),
            hook_bound_events: AtomicU64::new(0),
            overflow_count: AtomicU64::new(0),
            degraded_flags: AtomicU32::new(0),
            stop_reason: AtomicU32::new(0),
            stop_request_qpc: AtomicU64::new(0),
            physical_keys: std::array::from_fn(|_| AtomicU64::new(0)),
            conflict_key: AtomicU64::new(0),
        }
    }
}
#[repr(C)]
pub struct Ledger {
    pub held_keys: [AtomicU64; 4],
    pub held_mouse: AtomicU32,
    pub seq: AtomicU64,
}
impl Default for Ledger {
    fn default() -> Self {
        Self {
            held_keys: std::array::from_fn(|_| AtomicU64::new(0)),
            held_mouse: AtomicU32::new(0),
            seq: AtomicU64::new(0),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LedgerSnapshot {
    pub keys: [u64; 4],
    pub mouse: u32,
}
impl LedgerSnapshot {
    pub fn contains(&self, key: OutKey) -> bool {
        match key {
            OutKey::Key(k) => self.keys[usize::from(k.0) / 64] & (1u64 << (k.0 % 64)) != 0,
            OutKey::LeftButton => self.mouse & 1 != 0,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.keys.iter().all(|k| *k == 0) && self.mouse == 0
    }
    pub fn candidates(keys: &[bool; 256]) -> Self {
        let mut s = Self::default();
        for (i, held) in keys.iter().enumerate() {
            if *held {
                if i == 1 {
                    s.mouse |= 1;
                } else {
                    s.keys[i / 64] |= 1 << (i % 64);
                }
            }
        }
        s
    }
}
impl Ledger {
    // The Engine Driver is the sole writer. The odd/even sequence permits conservative fallback after a kill.
    fn set(&self, key: OutKey, down: bool) {
        self.seq.fetch_add(1, SeqCst);
        match key {
            OutKey::Key(k) => {
                let word = &self.held_keys[usize::from(k.0) / 64];
                let mask = 1u64 << (k.0 % 64);
                if down {
                    word.fetch_or(mask, SeqCst);
                } else {
                    word.fetch_and(!mask, SeqCst);
                }
            }
            OutKey::LeftButton => {
                if down {
                    self.held_mouse.fetch_or(1, SeqCst);
                } else {
                    self.held_mouse.fetch_and(!1, SeqCst);
                }
            }
        }
        self.seq.fetch_add(1, SeqCst);
    }
    pub fn before_send(&self, events: &[OutEvent]) {
        for e in events {
            match *e {
                OutEvent::KeyDown(k) => self.set(OutKey::Key(k), true),
                OutEvent::LeftDown => self.set(OutKey::LeftButton, true),
                _ => {}
            }
        }
    }
    pub fn after_success(&self, events: &[OutEvent]) {
        for e in events {
            match *e {
                OutEvent::KeyUp(k) => self.set(OutKey::Key(k), false),
                OutEvent::LeftUp => self.set(OutKey::LeftButton, false),
                _ => {}
            }
        }
    }
    pub fn snapshot(&self) -> Option<LedgerSnapshot> {
        let before = self.seq.load(SeqCst);
        if before & 1 != 0 {
            return None;
        }
        let s = LedgerSnapshot {
            keys: std::array::from_fn(|i| self.held_keys[i].load(SeqCst)),
            mouse: self.held_mouse.load(SeqCst),
        };
        if self.seq.load(SeqCst) == before {
            Some(s)
        } else {
            None
        }
    }
    pub fn contains(&self, key: Key) -> bool {
        self.held_keys[usize::from(key.0) / 64].load(SeqCst) & (1u64 << (key.0 % 64)) != 0
    }
}
/// Indices are part of layout v1: kind, session, action, stage, hook_enter, enqueue,
/// engine_receive, deadline, send_begin, send_end, inserted, expected, queue_depth,
/// config_version, geometry_version, source_seq.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogData(pub [u64; LOG_FIELDS]);
#[repr(C)]
pub struct LogRecord {
    pub seq: AtomicU64,
    pub fields: [AtomicU64; LOG_FIELDS],
}
impl Default for LogRecord {
    fn default() -> Self {
        Self {
            seq: AtomicU64::new(0),
            fields: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}
impl LogRecord {
    pub fn read(&self, expected: u64) -> Option<LogData> {
        if self.seq.load(SeqCst) != expected {
            return None;
        }
        let d = LogData(std::array::from_fn(|i| self.fields[i].load(SeqCst)));
        (self.seq.load(SeqCst) == expected).then_some(d)
    }
    fn write(&self, seq: u64, data: LogData) {
        self.seq.store(0, SeqCst);
        for (field, value) in self.fields.iter().zip(data.0) {
            field.store(value, SeqCst);
        }
        self.seq.store(seq, SeqCst);
    }
}
#[repr(C)]
pub struct Ring {
    pub write_seq: AtomicU64,
    pub entries: [LogRecord; RING_LEN],
}
impl Ring {
    /// Single writer: Engine only. SeqCst also invalidates a slot before overwriting its payload.
    pub fn push(&self, data: LogData) -> u64 {
        let seq = self
            .write_seq
            .load(SeqCst)
            .checked_add(1)
            .expect("log sequence exhausted");
        self.entries[(seq as usize - 1) % RING_LEN].write(seq, data);
        self.write_seq.store(seq, SeqCst);
        seq
    }
}
#[derive(Debug)]
pub struct RingReader {
    pub next: u64,
    pub lost: u64,
}
impl Default for RingReader {
    fn default() -> Self {
        Self { next: 1, lost: 0 }
    }
}
impl RingReader {
    pub fn read(&mut self, ring: &Ring) -> Option<(u64, LogData)> {
        let latest = ring.write_seq.load(SeqCst);
        let oldest = latest.saturating_sub(RING_LEN as u64 - 1).max(1);
        if self.next < oldest {
            self.lost += oldest - self.next;
            self.next = oldest;
        }
        if self.next > latest {
            return None;
        }
        let data = ring.entries[(self.next as usize - 1) % RING_LEN].read(self.next)?;
        let seq = self.next;
        self.next += 1;
        Some((seq, data))
    }
}
#[repr(C)]
pub struct SharedMemory {
    pub header: Header,
    pub progress: Progress,
    pub ledger: Ledger,
    pub ring: Ring,
}
impl SharedMemory {
    /// Initializes freshly allocated, aligned, exclusively owned memory without a 1 MB stack temporary.
    /// # Safety
    /// `ptr` must point to size_of::<Self>() writable aligned bytes, not be initialized or observed concurrently.
    pub unsafe fn initialize(ptr: *mut Self, session: SessionId, frequency: u64) {
        // SAFETY: Caller supplies valid exclusive aligned storage; each field is written before any reference is formed.
        unsafe {
            std::ptr::addr_of_mut!((*ptr).header).write(Header {
                magic: MAGIC,
                layout_version: LAYOUT_VERSION,
                size: size_of::<Self>() as u32,
                session: AtomicU32::new(session.0),
                config_version: AtomicU32::new(0),
                engine_state: AtomicU32::new(0),
                reserved: 0,
                qpc_frequency: frequency,
            });
            std::ptr::addr_of_mut!((*ptr).progress).write(Progress::default());
            std::ptr::addr_of_mut!((*ptr).ledger).write(Ledger::default());
            std::ptr::addr_of_mut!((*ptr).ring.write_seq).write(AtomicU64::new(0));
            let entries = std::ptr::addr_of_mut!((*ptr).ring.entries).cast::<LogRecord>();
            for i in 0..RING_LEN {
                entries.add(i).write(LogRecord::default());
            }
        }
    }
    pub fn boxed(session: SessionId, frequency: u64) -> Box<Self> {
        let mut b = Box::<Self>::new_uninit();
        // SAFETY: Box allocation is aligned, sized and exclusive; initialize writes every field before assume_init.
        unsafe {
            Self::initialize(b.as_mut_ptr(), session, frequency);
            b.assume_init()
        }
    }
}
const _: () = assert!(align_of::<SharedMemory>() == 8);
const _: () = assert!(size_of::<Header>() == 40);
const _: () = assert!(size_of::<Ledger>() == 48);
const _: () = assert!(size_of::<LogRecord>() == 136);
const _: () = assert!(std::mem::offset_of!(SharedMemory, header) == 0);
