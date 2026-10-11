#![cfg(windows)]
//! Marker width must survive the native mouse input path on 64-bit Windows.
use baauto_win_platform::random_marker;

#[test]
fn random_marker_fits_mouse_extra_info_width() {
    for _ in 0..64 {
        let marker = random_marker().unwrap();
        assert_ne!(marker, 0);
        assert!(u32::try_from(marker).is_ok());
    }
}
