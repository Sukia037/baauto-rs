#![cfg(windows)]
//! Interactive Windows desktop: start baauto-receiver.exe, focus it, then press W once.
//! cargo test -p baauto-bench --test windows_capture -- --ignored --nocapture --test-threads=1
//! Compares DXGI LastPresentTime/ROI against a baseline, without injecting input.
use baauto_bench::dxgi::Capture;
use baauto_config::*;
use baauto_win_platform::*;
use std::ffi::OsStr;
#[test]
#[ignore = "needs a focused fullscreen receiver and physical key press"]
fn receiver_dxgi_change_has_raw_qpc_and_exportable_roi() {
    let clock = Clock::new().unwrap();
    let until = clock.now().after(clock.rate.0 * 15);
    let target = loop {
        if let Some(target) = target_for_foreground(OsStr::new("baauto-receiver.exe")).unwrap() {
            break target;
        }
        assert!(clock.now() < until, "focus receiver");
        wait_control_period(clock, clock.rate.0 / 20).unwrap();
    };
    assert!(!target.warning());
    let mut capture = Capture::new(target.monitor).unwrap();
    let before = loop {
        if let Some(frame) = capture.next_frame(20).unwrap() {
            break frame;
        }
        assert!(clock.now() < until, "baseline frame timeout");
    };
    let roi = HandRect {
        x: [0.1, 0.2],
        y: [0.1, 0.2],
        calibrated: true,
    };
    let changed = loop {
        assert!(clock.now() < until, "press W on receiver after baseline");
        if let Some(frame) = capture.next_frame(20).unwrap()
            && frame.mean_difference(&before, roi).unwrap() > 10.
        {
            break frame;
        }
    };
    assert!(changed.present > before.present);
    assert!(changed.present <= clock.now());
    let path = std::env::temp_dir().join(format!("baauto-m5-{}.bmp", std::process::id()));
    std::fs::write(&path, changed.crop(roi).unwrap().bmp().unwrap()).unwrap();
    println!(
        "baseline={} changed={} ROI={}",
        before.present.0,
        changed.present.0,
        path.display()
    );
}
