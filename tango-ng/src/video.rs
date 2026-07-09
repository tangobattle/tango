//! CPU video filters — tango's GPU effects (video/effects/*.wgsl)
//! re-based on CPU implementations, applied to the RGBA framebuffer
//! before it's uploaded as a Slint image: hq2x/3x/4x via the `hqx`
//! crate, MMPX via the `mmpx` crate, and a small LCD-grille shader
//! equivalent. The GBA frame is 240×160, so even hq4x is well under a
//! millisecond of work per frame.

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    None,
    Hq2x,
    Hq3x,
    Hq4x,
    Mmpx,
    Lcd,
}

impl Filter {
    /// Config string ↔ filter, matching tango's `video_filter` values.
    pub fn from_config(s: &str) -> Self {
        match s {
            "hq2x" => Filter::Hq2x,
            "hq3x" => Filter::Hq3x,
            "hq4x" => Filter::Hq4x,
            "mmpx" => Filter::Mmpx,
            "lcd" => Filter::Lcd,
            _ => Filter::None,
        }
    }

    /// Settings-picker row order. Index 0 is the passthrough.
    pub const ALL: [Filter; 6] = [
        Filter::None,
        Filter::Hq2x,
        Filter::Hq3x,
        Filter::Hq4x,
        Filter::Mmpx,
        Filter::Lcd,
    ];

    pub fn config_name(self) -> &'static str {
        match self {
            Filter::None => "",
            Filter::Hq2x => "hq2x",
            Filter::Hq3x => "hq3x",
            Filter::Hq4x => "hq4x",
            Filter::Mmpx => "mmpx",
            Filter::Lcd => "lcd",
        }
    }

    /// Apply to an RGBA8 frame; `None` = passthrough (caller uploads
    /// the input as-is). Returns the scaled frame.
    pub fn apply(self, rgba: &[u8], w: u32, h: u32) -> Option<(u32, u32, Vec<u8>)> {
        match self {
            Filter::None => None,
            Filter::Hq2x => Some(hqx_scale(rgba, w, h, 2)),
            Filter::Hq3x => Some(hqx_scale(rgba, w, h, 3)),
            Filter::Hq4x => Some(hqx_scale(rgba, w, h, 4)),
            Filter::Mmpx => {
                let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())?;
                let out = mmpx::magnify(&img);
                let (ow, oh) = out.dimensions();
                Some((ow, oh, out.into_raw()))
            }
            Filter::Lcd => Some(lcd_scale(rgba, w, h)),
        }
    }
}

/// hq2x/3x/4x. The hqx crate takes 0xAARRGGBB words, so convert from
/// RGBA bytes explicitly on the way in and back on the way out (no
/// byte-order punning to get subtly wrong).
fn hqx_scale(rgba: &[u8], w: u32, h: u32, factor: u32) -> (u32, u32, Vec<u8>) {
    let src: Vec<u32> = rgba
        .chunks_exact(4)
        .map(|p| ((p[3] as u32) << 24) | ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | p[2] as u32)
        .collect();
    let (ow, oh) = (w * factor, h * factor);
    let mut dst = vec![0u32; (ow * oh) as usize];
    match factor {
        2 => hqx::hq2x(&src, &mut dst, w, h),
        3 => hqx::hq3x(&src, &mut dst, w, h),
        _ => hqx::hq4x(&src, &mut dst, w, h),
    }
    let mut out = Vec::with_capacity(dst.len() * 4);
    for p in dst {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]);
    }
    (ow, oh, out)
}

/// LCD grille at 3×: each source pixel becomes a 3×3 cell whose columns
/// emphasize R / G / B in turn (the other channels attenuated), with the
/// bottom row dimmed as the cell seam — the CPU shape of lcd.wgsl.
fn lcd_scale(rgba: &[u8], w: u32, h: u32) -> (u32, u32, Vec<u8>) {
    const SUB: [[u32; 3]; 3] = [[256, 96, 96], [96, 256, 96], [96, 96, 256]];
    const ROW: [u32; 3] = [256, 256, 176];
    let (ow, oh) = (w * 3, h * 3);
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let s = ((y * w + x) * 4) as usize;
            let (r, g, b, a) = (
                rgba[s] as u32,
                rgba[s + 1] as u32,
                rgba[s + 2] as u32,
                rgba[s + 3],
            );
            for dy in 0..3u32 {
                let row = ROW[dy as usize];
                for dx in 0..3u32 {
                    let sub = SUB[dx as usize];
                    let d = (((y * 3 + dy) * ow + x * 3 + dx) * 4) as usize;
                    out[d] = ((r * sub[0] * row) >> 16).min(255) as u8;
                    out[d + 1] = ((g * sub[1] * row) >> 16).min(255) as u8;
                    out[d + 2] = ((b * sub[2] * row) >> 16).min(255) as u8;
                    out[d + 3] = a;
                }
            }
        }
    }
    (ow, oh, out)
}
