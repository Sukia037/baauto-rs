//! RegisterHotKey matches an exact modifier set. Expand AtLeast to its finite supersets.
#![forbid(unsafe_code)]
use arrayvec::ArrayVec;
use baauto_config::{Chord, ModsMatch};
pub fn global_hotkey_masks(chord: Chord) -> ArrayVec<u8, 16> {
    let mut masks = ArrayVec::new();
    for actual in 0..16 {
        if match chord.mods {
            ModsMatch::AtLeast(required) => actual & required == required,
            ModsMatch::Exact(required) => actual == required,
        } {
            masks.push(actual);
        }
    }
    masks
}
