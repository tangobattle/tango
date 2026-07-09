//! The read-only save viewer's data layer: a stripped port of
//! `tango/src/selection.rs`'s `Loaded` (ROM assets + the one-time image
//! bake, with `slint::Image` handles in place of iced ones) plus the
//! model-building halves of `tango/src/save_view/{folder.rs,navi/mod.rs}`
//! (the layouts live in `ui/app.slint`). No editors, and no
//! `rom_overrides`: tango-ng's `patch.rs` deliberately doesn't parse the
//! text overrides yet, so a patched save shows whatever the BPS itself
//! rewrote in the ROM.

use crate::rom::GameRef;
use crate::{ChipRow, NaviHeader};
use std::collections::HashMap;

/// Number of chip slots in an equipped folder.
const MAX_FOLDER_CHIPS: usize = 30;

/// Currently selected game + save + their derived ROM assets and
/// pre-baked sprite images. Rebuilt only when the (game, patch, save)
/// selection changes, so pushing view models stays cheap.
pub struct Loaded {
    pub save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    pub assets: Box<dyn tango_dataview::rom::Assets + Send + Sync>,
    /// Chip icons, cropped to their visible 14×14 (from the 16×16
    /// sprite). Indexed by chip id; `None` = no such chip.
    pub chip_icons: Vec<Option<slint::Image>>,
    /// Full-size chip artwork (native dimensions, e.g. 56×48 on BN6),
    /// baked for the hover-preview popover (a follow-up; unused until
    /// that lands).
    #[allow(dead_code)]
    pub chip_full_images: Vec<Option<slint::Image>>,
    /// Element icons by element id, same 14×14 crop as chip icons.
    pub element_icons: HashMap<usize, slint::Image>,
    /// Navi emblems (roster games only), cropped to 15×15 from (1,0).
    pub navi_emblems: HashMap<usize, slint::Image>,
}

impl Loaded {
    /// Build from the *raw* (unpatched) ROM, applying the selected
    /// patch from disk first — the viewer bakes from exactly what Play
    /// would boot. On apply failure we fall back to the unpatched ROM
    /// (and log) so the view still renders, mirroring tango's
    /// `Loaded::build`.
    ///
    /// Measured at well under 50ms even for BN6 (the largest chip
    /// library), so this runs inline on the selection callback; the
    /// debug log below keeps the number observable.
    pub fn build(
        game: GameRef,
        rom: &[u8],
        save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        patches_path: &std::path::Path,
        patch: Option<(String, semver::Version)>,
    ) -> Self {
        let started = std::time::Instant::now();
        let rom = match patch {
            Some((name, version)) => {
                match crate::patch::apply_patch_from_disk(rom, game, patches_path, &name, &version) {
                    Ok(patched) => patched,
                    Err(e) => {
                        log::error!(
                            "failed to apply patch {name} v{version} to {:?}: {e}",
                            game.family_and_variant()
                        );
                        rom.to_vec()
                    }
                }
            }
            None => rom.to_vec(),
        };

        let wram = save.as_raw_wram().into_owned();
        let assets = game.load_rom_assets(&rom, &wram, None);

        // Chip icons (14×14 cropped from 16×16) + full chip artwork.
        // Pre-pass once at load time so model rebuilds just clone
        // handles (same shape as tango/src/selection.rs:216-250).
        let mut chip_icons: Vec<Option<slint::Image>> = Vec::with_capacity(assets.num_chips());
        let mut chip_full_images: Vec<Option<slint::Image>> = Vec::with_capacity(assets.num_chips());
        for id in 0..assets.num_chips() {
            let chip = assets.chip(id);
            chip_icons.push(chip.as_ref().map(|c| cropped_image(&c.icon(), 1, 1, 14, 14)));
            chip_full_images.push(chip.as_ref().map(|c| slint_image(c.image())));
        }

        // Element icons: element ids are sparse; try the small id space.
        let mut element_icons = HashMap::new();
        for id in 0..32 {
            if let Some(img) = assets.element_icon(id) {
                element_icons.insert(id, cropped_image(&img, 1, 1, 14, 14));
            }
        }

        // Navi emblems for the link-navi roster games: 15×15 from (1,0).
        let mut navi_emblems = HashMap::new();
        for id in 0..assets.num_navis() {
            if let Some(navi) = assets.navi(id) {
                navi_emblems.insert(id, cropped_image(&navi.emblem(), 1, 0, 15, 15));
            }
        }

        log::debug!(
            "save-view assets baked for {:?} in {:?}",
            game.family_and_variant(),
            started.elapsed()
        );
        Self {
            save,
            assets,
            chip_icons,
            chip_full_images,
            element_icons,
            navi_emblems,
        }
    }
}

/// image::RgbaImage → slint::Image, same SharedPixelBuffer route as the
/// emulator frame pump in main.rs.
fn slint_image(img: image::RgbaImage) -> slint::Image {
    let (w, h) = img.dimensions();
    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
    buf.make_mut_bytes().copy_from_slice(img.as_raw());
    slint::Image::from_rgba8(buf)
}

/// Crop-then-convert helper (tango's `cropped_handle`, selection.rs:477).
fn cropped_image(src: &image::RgbaImage, x: u32, y: u32, w: u32, h: u32) -> slint::Image {
    slint_image(image::imageops::crop_imm(src, x, y, w, h).to_image())
}

/// Accent color for the left edge of a chip row; `None` = no accent
/// (a default chip with no class adornment). Colors from tango's
/// save_view/folder.rs `class_accent`.
fn class_accent(class: Option<tango_dataview::rom::ChipClass>, dark: bool) -> Option<slint::Color> {
    if dark {
        return Some(slint::Color::from_rgb_u8(0x4a, 0x55, 0x82));
    }
    match class {
        Some(tango_dataview::rom::ChipClass::Mega) => Some(slint::Color::from_rgb_u8(0x52, 0x84, 0x9c)),
        Some(tango_dataview::rom::ChipClass::Giga) => Some(slint::Color::from_rgb_u8(0xc4, 0x52, 0x84)),
        _ => None,
    }
}

/// Per-group metadata accumulated while collapsing folder slots
/// (tango's `GroupedChip`; the two tag flags fold into a count so the
/// row can render one TAG badge per tagged copy).
#[derive(Default)]
struct GroupedChip {
    count: usize,
    is_regular: bool,
    tag_count: i32,
}

/// The equipped folder as display rows (tango's `render_folder`, data
/// half). `grouped` collapses by chip identity — ordered dedup by
/// (id, code), 30 entries so a linear find on a Vec does — while
/// per-slot mode emits one row per *filled* slot (empty slots are
/// skipped, like tango; grouped mode keeps the empty-slot group as a
/// single "—" row). Chip names/codes come from the ROM, so no language
/// is needed here — the surrounding labels live in the `I18n` global.
pub fn folder_rows(loaded: &Loaded, grouped: bool) -> Vec<ChipRow> {
    let Some(chips_view) = loaded.save.view_chips() else {
        return Vec::new();
    };
    let assets = loaded.assets.as_ref();
    let folder_idx = chips_view.equipped_folder_index();
    // Read-only display treats "unsupported" and "unset" the same —
    // flatten the outer Option away.
    let regular_idx = chips_view.regular_chip_index(folder_idx).flatten();
    let tag_idxs = chips_view.tag_chip_indexes(folder_idx).flatten();

    let mut items: Vec<(Option<tango_dataview::save::Chip>, GroupedChip)> = Vec::new();
    for i in 0..MAX_FOLDER_CHIPS {
        let chip = chips_view.chip(folder_idx, i);
        if !grouped && chip.is_none() {
            continue;
        }
        let slot = if grouped {
            match items.iter().position(|(c, _)| *c == chip) {
                Some(pos) => pos,
                None => {
                    items.push((chip, GroupedChip::default()));
                    items.len() - 1
                }
            }
        } else {
            items.push((chip, GroupedChip::default()));
            items.len() - 1
        };
        let g = &mut items[slot].1;
        g.count += 1;
        if regular_idx == Some(i) {
            g.is_regular = true;
        }
        if let Some(t) = tag_idxs {
            g.tag_count += (t[0] == i) as i32 + (t[1] == i) as i32;
        }
    }

    items
        .into_iter()
        .map(|(chip, g)| {
            let info = chip.as_ref().and_then(|c| assets.chip(c.id));
            let class = info.as_ref().map(|i| i.class());
            let dark = info.as_ref().map(|i| i.dark()).unwrap_or(false);
            let accent = class_accent(class, dark);
            let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
            let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);
            let name = match (&chip, info.as_ref().and_then(|i| i.name())) {
                (None, _) => "—".to_string(),
                (Some(_), Some(name)) => name,
                (Some(_), None) => "???".to_string(),
            };
            ChipRow {
                icon: chip
                    .as_ref()
                    .and_then(|c| loaded.chip_icons.get(c.id).cloned().flatten())
                    .unwrap_or_default(),
                element_icon: info
                    .as_ref()
                    .and_then(|i| loaded.element_icons.get(&i.element()).cloned())
                    .unwrap_or_default(),
                name: name.into(),
                code: chip.as_ref().map(|c| c.code.to_string()).unwrap_or_default().into(),
                // Zero attack / zero MB render as blanks, not "0"s.
                power: if power > 0 { power.to_string() } else { String::new() }.into(),
                mb: if mb > 0 { format!("{mb}MB") } else { String::new() }.into(),
                count: format!("{}×", g.count).into(),
                is_regular: g.is_regular,
                tag_count: g.tag_count,
                accent: accent.unwrap_or_default(),
                has_accent: accent.is_some(),
                is_empty: chip.is_none(),
                description: info.as_ref().and_then(|i| i.description()).unwrap_or_default().into(),
            }
        })
        .collect()
}

/// The equipped-navi header card's content (tango's
/// `navi_card_content`, navi/mod.rs). Roster games (BN5/BN6/EXE4.5)
/// get emblem + name; BN1–4 report a placeholder navi the ROM has no
/// entry for, so they just show the HP. Buster levels (BN6 only) are
/// displayed exactly as `buster_stats` reports them — those are the
/// status-screen levels (1–5) already; tango's card does no further +1.
pub fn navi_header(loaded: &Loaded) -> NaviHeader {
    let assets = loaded.assets.as_ref();
    let navi = loaded.save.view_navi();
    let navi_id = navi.as_ref().map(|nv| nv.navi());
    let hp = navi.as_ref().map(|nv| nv.max_hp(assets));
    let buster = navi.as_ref().and_then(|nv| nv.buster_stats(assets));
    let roster_navi = navi_id.filter(|&id| assets.navi(id).is_some());
    let name = roster_navi.map(|id| {
        assets
            .navi(id)
            .and_then(|n| n.name())
            .unwrap_or_else(|| format!("Navi #{id}"))
    });
    NaviHeader {
        emblem: roster_navi
            .and_then(|id| loaded.navi_emblems.get(&id).cloned())
            .unwrap_or_default(),
        has_name: name.is_some(),
        name: name.unwrap_or_default().into(),
        hp: hp.map(|h| h.to_string()).unwrap_or_default().into(),
        buster_attack: buster.map(|b| b.attack.to_string()).unwrap_or_default().into(),
        buster_rapid: buster.map(|b| b.speed.to_string()).unwrap_or_default().into(),
        buster_charge: buster.map(|b| b.charge.to_string()).unwrap_or_default().into(),
        has_buster: buster.is_some(),
    }
}
