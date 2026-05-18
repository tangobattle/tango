//! Software upscale filters applied to each emulator frame
//! before it's uploaded to the GPU. Ported from
//! `tango/src/video.rs`. The selected filter is owned by the
//! session module, which pipes its output into iced's
//! `image::Handle::from_rgba`.
//!
//! `NullFilter` is the always-available pass-through.

pub mod hqx;
pub mod mmpx;

pub trait Filter {
    fn output_size(&self, size: [usize; 2]) -> [usize; 2];
    fn apply(&self, input: &[u8], output: &mut [u8], size: [usize; 2]);
}

pub struct NullFilter;
impl Filter for NullFilter {
    fn output_size(&self, size: [usize; 2]) -> [usize; 2] {
        size
    }
    fn apply(&self, input: &[u8], output: &mut [u8], _size: [usize; 2]) {
        output.copy_from_slice(input)
    }
}

/// Same registry as the legacy app. Empty string = "null" =
/// nearest-neighbor pass-through. Unknown names return `None`.
pub fn filter_by_name(name: &str) -> Option<Box<dyn Filter + Sync + Send>> {
    match name {
        "null" | "" => Some(Box::new(NullFilter)),
        "hq2x" => Some(Box::new(hqx::HQ2XFilter)),
        "hq3x" => Some(Box::new(hqx::HQ3XFilter)),
        "hq4x" => Some(Box::new(hqx::HQ4XFilter)),
        "mmpx" => Some(Box::new(mmpx::MMPXFilter)),
        _ => None,
    }
}

/// Display names of every filter, in pick-list order. The first
/// entry is the canonical "no filter" label.
pub const FILTERS: &[(&str, &str)] = &[
    ("", "None"),
    ("hq2x", "HQ2x"),
    ("hq3x", "HQ3x"),
    ("hq4x", "HQ4x"),
    ("mmpx", "MMPX"),
];
