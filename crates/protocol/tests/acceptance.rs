use baauto_core::{Key, OutEvent, OutKey, SessionId};
use baauto_protocol::shm::*;
use std::sync::{Arc, atomic::Ordering::SeqCst};
#[test]
fn shm_layout_has_no_pointers() {
    let s = SharedMemory::boxed(SessionId(9), 10_000_000);
    assert!(s.header.valid(SessionId(9)));
    assert!(!s.header.valid(SessionId(8)));
    assert_eq!(std::mem::align_of::<SharedMemory>(), 8);
    assert_eq!(std::mem::size_of::<LogRecord>(), 136);
    const {
        assert!(RING_LEN >= 8192);
    }
}
#[test]
fn slow_reader_counts_overwrites() {
    let s: Arc<SharedMemory> = Arc::from(SharedMemory::boxed(SessionId(1), 1));
    let w = Arc::clone(&s);
    let thread = std::thread::spawn(move || {
        for n in 1..=RING_LEN + 37 {
            w.ring.push(LogData([n as u64; 16]));
        }
    });
    thread.join().unwrap();
    let mut reader = RingReader::default();
    let mut count = 0;
    while let Some((seq, data)) = reader.read(&s.ring) {
        assert!(data.0.iter().all(|v| *v == seq));
        count += 1;
    }
    assert_eq!(reader.lost, 37);
    assert_eq!(count, RING_LEN);
}
#[test]
fn concurrent_ring_reader_never_observes_mixed_fields() {
    let s: Arc<SharedMemory> = Arc::from(SharedMemory::boxed(SessionId(1), 1));
    let w = Arc::clone(&s);
    let t = std::thread::spawn(move || {
        for n in 1..=RING_LEN * 3 {
            w.ring.push(LogData([n as u64; 16]));
        }
    });
    let mut r = RingReader::default();
    while !t.is_finished() || r.next <= s.ring.write_seq.load(SeqCst) {
        if let Some((seq, d)) = r.read(&s.ring) {
            assert!(d.0.iter().all(|v| *v == seq));
        } else {
            std::thread::yield_now();
        }
    }
    t.join().unwrap();
    assert_eq!(r.next - 1, s.ring.write_seq.load(SeqCst));
}
#[test]
fn ledger_short_send_does_not_clear_up() {
    let l = Ledger::default();
    l.before_send(&[OutEvent::KeyDown(Key(49)), OutEvent::LeftDown]);
    assert!(l.snapshot().unwrap().contains(OutKey::LeftButton));
    assert!(l.snapshot().unwrap().contains(OutKey::Key(Key(49))));
    l.before_send(&[OutEvent::LeftUp]);
    assert!(!l.snapshot().unwrap().is_empty());
    l.after_success(&[OutEvent::KeyUp(Key(49)), OutEvent::LeftUp]);
    assert!(l.snapshot().unwrap().is_empty());
}
#[test]
fn wake_protocol_never_sleeps_with_pending_events() {
    loom::model(|| {
        use loom::sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering::SeqCst},
        };
        let q = Arc::new(AtomicUsize::new(0));
        let event = Arc::new(Mutex::new(false));
        let q2 = q.clone();
        let e2 = event.clone();
        let producer = loom::thread::spawn(move || {
            q2.fetch_add(1, SeqCst);
            *e2.lock().unwrap() = true;
        });
        // The mutex models the kernel Event's signal/consume synchronization, not a resettable AtomicBool.
        let signaled = {
            let mut e = event.lock().unwrap();
            let value = *e;
            *e = false;
            value
        };
        if signaled && q.load(SeqCst) > 0 {
            q.fetch_sub(1, SeqCst);
        }
        let sleeping = q.load(SeqCst) == 0;
        producer.join().unwrap();
        assert!(!sleeping || q.load(SeqCst) == 0 || *event.lock().unwrap());
    });
}
#[test]
fn ledger_is_superset_at_any_kill_point() {
    for kill in 0..=4 {
        loom::model(move || {
            use loom::sync::{
                Arc,
                atomic::{AtomicBool, Ordering::SeqCst},
            };
            let ledger = Arc::new(AtomicBool::new(false));
            let actual = Arc::new(AtomicBool::new(false));
            let l = ledger.clone();
            let a = actual.clone();
            let t = loom::thread::spawn(move || {
                if kill == 0 {
                    return;
                }
                l.store(true, SeqCst);
                if kill == 1 {
                    return;
                }
                a.store(true, SeqCst);
                if kill == 2 {
                    return;
                }
                a.store(false, SeqCst);
                if kill == 3 {
                    return;
                }
                l.store(false, SeqCst);
            });
            t.join().unwrap();
            assert!(!actual.load(SeqCst) || ledger.load(SeqCst));
        });
    }
}
#[test]
fn ring_reader_detects_torn_record() {
    loom::model(|| {
        use loom::sync::{
            Arc,
            atomic::{AtomicU64, Ordering::SeqCst},
        };
        let seq = Arc::new(AtomicU64::new(1));
        let a = Arc::new(AtomicU64::new(1));
        let b = Arc::new(AtomicU64::new(1));
        let (s, x, y) = (seq.clone(), a.clone(), b.clone());
        let t = loom::thread::spawn(move || {
            s.store(0, SeqCst);
            x.store(2, SeqCst);
            y.store(2, SeqCst);
            s.store(2, SeqCst);
        });
        let before = seq.load(SeqCst);
        let x = a.load(SeqCst);
        let y = b.load(SeqCst);
        let after = seq.load(SeqCst);
        if before != 0 && before == after {
            assert_eq!(x, y);
            assert_eq!(x, before);
        }
        t.join().unwrap();
    });
}
