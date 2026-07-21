//! The web analog of the desktop's `selection::Loaded`: the committed
//! game + save + their derived assets, plus every image the save view
//! draws pre-baked to a PNG data URL (the browser's stand-in for the
//! desktop's iced image handles). Rebuilt only when the selection
//! changes, so per-render work stays cheap.

use std::collections::HashMap;

use base64::Engine as _;

use crate::library::GameRef;
use crate::rom_overrides::{Overrides, OverridenAssets};

/// Which sections of a loaded save can be edited in place. Each flag is a
/// pure capability probe — `view_*_mut().is_some()` — which needs `&mut save`,
/// so it's computed once and cached (the read-only `view_*()` probes answer a
/// different question: BN3 has a viewable-but-not-writable navicust, BN1–4 a
/// viewable-but-not-writable navi). Swapping the equipped navi flips some of
/// these, so re-probe via [`Loaded::refresh_editability`] after any in-memory
/// edit that can change capability.
#[derive(Clone, Copy, Default)]
pub struct Editability {
    /// `view_chips_mut().is_some()` — drives the Folder editor.
    pub folder: bool,
    /// `view_navicust_mut().is_some()` (BN4/5/6, and not a link navi).
    pub navicust: bool,
    /// `view_navi_mut().is_some()` — the equipped navi (BN5/BN6/BN4.5).
    pub navi: bool,
    /// `view_patch_cards_mut().is_some()` — BN4 (PatchCard4s, slot-based)
    /// and BN5/BN6 (PatchCard56s, list-based).
    pub patch_cards: bool,
    /// `view_auto_battle_data_mut().is_some()` (BN4/BN5).
    pub auto_battle_data: bool,
}

impl Editability {
    /// Probe every section's writable view once. Constructing a mutable view
    /// has no side effects, so this is a pure capability check.
    fn probe(save: &mut (dyn tango_dataview::save::Save + Send + Sync)) -> Self {
        // Each `is_some()` gets its own statement so the borrowed view
        // temporary is dropped before the next probe.
        let folder = save.view_chips_mut().is_some();
        let navicust = save.view_navicust_mut().is_some();
        let navi = save.view_navi_mut().is_some();
        let patch_cards = save.view_patch_cards_mut().is_some();
        let auto_battle_data = save.view_auto_battle_data_mut().is_some();
        Self {
            folder,
            navicust,
            navi,
            patch_cards,
            auto_battle_data,
        }
    }

    /// Whether *any* section is editable — drives the single save-level
    /// Edit button.
    pub fn any(&self) -> bool {
        self.folder || self.navicust || self.navi || self.patch_cards || self.auto_battle_data
    }
}

pub struct Loaded {
    #[allow(dead_code)] // the replays tab's save-view embedding
    pub game: GameRef,
    /// The OPFS `saves/` file this save came from — the commit path
    /// writes the edited SRAM back to it.
    pub save_file: String,
    pub save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    /// Which sections of this save can be edited in place.
    pub editability: Editability,
    /// Patch (name, version) baked into this Loaded, if any. `None` = raw ROM.
    #[allow(dead_code)] // the replays tab's save-view embedding
    pub patch: Option<(String, semver::Version)>,
    pub assets: Box<dyn tango_dataview::rom::Assets + Send + Sync>,
    /// 14×14 chip icons as data URLs, indexed by chip id.
    pub chip_icons: Vec<Option<String>>,
    /// Full-size chip images (native w, h, data URL) for hover previews.
    pub chip_images: Vec<Option<(u32, u32, String)>>,
    pub element_icons: HashMap<usize, String>,
    /// 15×15 navi emblems as data URLs, indexed by navi id.
    pub navi_emblems: HashMap<usize, String>,
    /// Signature color per navi (`#rrggbb`), extracted from its emblem
    /// pixels — tints the navi picker's plates. Missing when the emblem
    /// is entirely monochrome.
    pub navi_accents: HashMap<usize, String>,
}

impl Loaded {
    /// Build from a ROM that's already had its patch applied. `overrides`
    /// come from the patch version's `info.toml` (`Overrides::default()`
    /// for a raw ROM); the charset override is fed into the asset load and
    /// the name/description overrides wrap the assets, exactly like the
    /// desktop's `selection::Loaded`.
    pub fn build(
        game: GameRef,
        rom: &[u8],
        save_file: String,
        save_bytes: &[u8],
        patch: Option<(String, semver::Version)>,
        overrides: Overrides,
    ) -> anyhow::Result<Self> {
        let mut save = game.parse_save(save_bytes)?;
        let editability = Editability::probe(&mut *save);

        let wram = save.as_raw_wram().into_owned();
        let charset_owned: Option<Vec<&str>> = overrides
            .charset
            .as_ref()
            .map(|c| c.iter().map(|s| s.as_str()).collect());
        let inner = game.load_rom_assets(rom, &wram, charset_owned.as_deref());
        let assets: Box<dyn tango_dataview::rom::Assets + Send + Sync> = Box::new(OverridenAssets::new(inner, overrides));

        // Chip icons (14x14 cropped from 16x16) + full chip images for
        // hover previews. Baked once at load time so renders just clone
        // data-URL strings.
        let mut chip_icons: Vec<Option<String>> = Vec::with_capacity(assets.num_chips());
        let mut chip_images: Vec<Option<(u32, u32, String)>> = Vec::with_capacity(assets.num_chips());
        for id in 0..assets.num_chips() {
            let chip = assets.chip(id);
            chip_icons.push(chip.as_ref().and_then(|c| cropped_data_url(&c.icon(), 1, 1, 14, 14)));
            chip_images.push(chip.as_ref().and_then(|c| {
                let img = c.image();
                let (w, h) = (img.width(), img.height());
                png_data_url(&img).map(|url| (w, h, url))
            }));
        }

        // Element icons: element ids are sparse; try the small id space.
        let mut element_icons = HashMap::new();
        for id in 0..32 {
            if let Some(img) = assets.element_icon(id) {
                if let Some(url) = cropped_data_url(&img, 1, 1, 14, 14) {
                    element_icons.insert(id, url);
                }
            }
        }

        // Navi emblems for LinkNavi games: 15x15 from (1,0). The accent
        // color (most prominent saturated pixel color) is pulled from the
        // same crop for the navi picker's plate tinting.
        let mut navi_emblems = HashMap::new();
        let mut navi_accents = HashMap::new();
        for id in 0..assets.num_navis() {
            if let Some(navi) = assets.navi(id) {
                let crop = image::imageops::crop_imm(&navi.emblem(), 1, 0, 15, 15).to_image();
                if let Some(accent) = emblem_accent(&crop) {
                    navi_accents.insert(id, accent);
                }
                if let Some(url) = png_data_url(&crop) {
                    navi_emblems.insert(id, url);
                }
            }
        }

        Ok(Self {
            game,
            save_file,
            save,
            editability,
            patch,
            assets,
            chip_icons,
            chip_images,
            element_icons,
            navi_emblems,
            navi_accents,
        })
    }

    /// Re-probe section [`Editability`] from the current in-memory save.
    /// Swapping the equipped navi flips navicust / patch-card capability,
    /// so the edit path calls this after a navi change.
    pub fn refresh_editability(&mut self) {
        self.editability = Editability::probe(&mut *self.save);
    }
}

/// Encode an RGBA image as a PNG data URL.
pub fn png_data_url(img: &image::RgbaImage) -> Option<String> {
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    ))
}

fn cropped_data_url(src: &image::RgbaImage, x: u32, y: u32, w: u32, h: u32) -> Option<String> {
    let sub = image::imageops::crop_imm(src, x, y, w, h).to_image();
    png_data_url(&sub)
}

/// The emblem's signature color: every distinct opaque pixel color is
/// scored by frequency, weighted toward saturated mid-to-bright tones so
/// black outlines and white highlights lose to the emblem's actual
/// identity color. `None` when nothing scores.
fn emblem_accent(img: &image::RgbaImage) -> Option<String> {
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for p in img.pixels() {
        if p.0[3] >= 0x80 {
            *counts.entry([p.0[0], p.0[1], p.0[2]]).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .filter_map(|(rgb, n)| {
            let max = rgb.iter().copied().max().unwrap() as f32 / 255.0;
            let min = rgb.iter().copied().min().unwrap() as f32 / 255.0;
            let saturation = if max > 0.0 { (max - min) / max } else { 0.0 };
            // Saturation dominates; the small constant keeps a vivid-but-
            // rare color from losing to a huge near-gray field, and the
            // value factor zeroes out the black outline entirely.
            let score = n as f32 * (saturation + 0.05) * max;
            (score > 0.0).then_some((score, rgb))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, rgb)| format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]))
}
