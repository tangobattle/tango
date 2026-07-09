//! The read-only save viewer's data layer: a stripped port of
//! `tango/src/selection.rs`'s `Loaded` (ROM assets + the one-time image
//! bake, with `slint::Image` handles in place of iced ones) plus the
//! model-building halves of `tango/src/save_view/*` — folder, navi
//! header, navicust (grid baked via [`crate::navicust`]), patch cards,
//! and auto battle data (the layouts live in `ui/app.slint`), and the
//! sections' copy-as-text renderings. No editors, and no
//! `rom_overrides`: tango-ng's `patch.rs` deliberately doesn't parse the
//! text overrides yet, so a patched save shows whatever the BPS itself
//! rewrote in the ROM.

use crate::rom::GameRef;
use crate::{AbdRow, ChipRow, NaviHeader, NcpPartRow, PatchCardLine};
use std::collections::HashMap;

/// Number of chip slots in an equipped folder.
pub const MAX_FOLDER_CHIPS: usize = 30;

/// Which sections of a save can be edited in place — one writable-view
/// probe per section (tango's selection.rs Editability).
pub struct Editability {
    pub folder: bool,
    pub navicust: bool,
    pub navi: bool,
    pub patch_cards: bool,
    pub auto_battle_data: bool,
}

impl Editability {
    /// Probe every section's writable view once (pure capability check).
    pub(crate) fn probe(save: &mut (dyn tango_dataview::save::Save + Send + Sync)) -> Self {
        // One statement per probe so each mutable borrow drops before
        // the next.
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

    /// Whether *any* section is editable — drives the save-level Edit
    /// button (the folder editor is the one ported so far, so it also
    /// gates on `folder` at the call site).
    pub fn any(&self) -> bool {
        self.folder || self.navicust || self.navi || self.patch_cards || self.auto_battle_data
    }
}

/// Currently selected game + save + their derived ROM assets and
/// pre-baked sprite images. Rebuilt only when the (game, patch, save)
/// selection changes, so pushing view models stays cheap.
pub struct Loaded {
    pub game: GameRef,
    pub save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    /// Which sections can be edited in place (probed once at build).
    pub editability: Editability,
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

        let mut save = save;
        let editability = Editability::probe(&mut *save);
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
            game,
            save,
            editability,
            assets,
            chip_icons,
            chip_full_images,
            element_icons,
            navi_emblems,
        }
    }
}

/// The Cover tab's logos, pre-positioned in Rust (tango's
/// save_view/cover.rs): a single logo renders as a centered banner;
/// twin-version families stack with a left/right stagger the way the
/// Legacy Collection lays out twin covers. Coordinates are display px
/// inside a `(lane_w, lane_h)` box the layout centers in the pane.
pub struct CoverModel {
    pub logos: Vec<crate::CoverLogo>,
    pub lane_w: f32,
    pub lane_h: f32,
}

pub fn cover_model(loaded: &Loaded) -> CoverModel {
    // The loaded game's own variant first; any sibling variants in the
    // family follow (Gregar/Falzar etc. fan both out).
    let (family, variant) = loaded.game.family_and_variant();
    let mut order: Vec<GameRef> = vec![loaded.game];
    order.extend(crate::game::games_in_family(family).filter(|g| g.family_and_variant().1 != variant));
    let images: Vec<image::RgbaImage> = order.iter().map(|g| g.logo_image.to_rgba8()).collect();

    match images.as_slice() {
        [top, bottom, ..] => {
            const H: f32 = 140.0;
            const STAGGER: f32 = 64.0;
            const GAP: f32 = 20.0;
            let disp_w = |img: &image::RgbaImage| H * img.width() as f32 / img.height() as f32;
            let (top_w, bottom_w) = (disp_w(top), disp_w(bottom));
            // Shared lane so the pair centers as a unit: top logo hugs
            // the lane's left edge, bottom logo its right edge.
            let lane_w = top_w.max(bottom_w) + STAGGER;
            CoverModel {
                logos: vec![
                    crate::CoverLogo {
                        img: slint_image(top.clone()),
                        x: 0.0,
                        y: 0.0,
                        w: top_w,
                        h: H,
                    },
                    crate::CoverLogo {
                        img: slint_image(bottom.clone()),
                        x: lane_w - bottom_w,
                        y: H + GAP,
                        w: bottom_w,
                        h: H,
                    },
                ],
                lane_w,
                lane_h: H * 2.0 + GAP,
            }
        }
        [only] => {
            const H: f32 = 220.0;
            let w = H * only.width() as f32 / only.height() as f32;
            CoverModel {
                logos: vec![crate::CoverLogo {
                    img: slint_image(only.clone()),
                    x: 0.0,
                    y: 0.0,
                    w,
                    h: H,
                }],
                lane_w: w,
                lane_h: H,
            }
        }
        [] => CoverModel {
            logos: Vec::new(),
            lane_w: 0.0,
            lane_h: 0.0,
        },
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
            make_chip_row(
                loaded,
                chip.as_ref().map(|c| c.id),
                chip.as_ref().map(|c| c.code.to_string()),
                &g,
            )
        })
        .collect()
}

/// One [`ChipRow`] from a chip id (+ optional code) and its group
/// metadata — the shared row builder behind the Folder list and the
/// Auto Battle Data deck (which has ids but no codes).
fn make_chip_row(loaded: &Loaded, id: Option<usize>, code: Option<String>, g: &GroupedChip) -> ChipRow {
    let assets = loaded.assets.as_ref();
    let info = id.and_then(|id| assets.chip(id));
    let class = info.as_ref().map(|i| i.class());
    let dark = info.as_ref().map(|i| i.dark()).unwrap_or(false);
    let accent = class_accent(class, dark);
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);
    let name = match (id, info.as_ref().and_then(|i| i.name())) {
        (None, _) => "—".to_string(),
        (Some(_), Some(name)) => name,
        (Some(_), None) => "???".to_string(),
    };
    ChipRow {
        icon: id
            .and_then(|id| loaded.chip_icons.get(id).cloned().flatten())
            .unwrap_or_default(),
        element_icon: info
            .as_ref()
            .and_then(|i| loaded.element_icons.get(&i.element()).cloned())
            .unwrap_or_default(),
        name: name.into(),
        code: code.unwrap_or_default().into(),
        // Zero attack / zero MB render as blanks, not "0"s.
        power: if power > 0 { power.to_string() } else { String::new() }.into(),
        mb: if mb > 0 { format!("{mb}MB") } else { String::new() }.into(),
        count: format!("{}×", g.count).into(),
        is_regular: g.is_regular,
        tag_count: g.tag_count,
        accent: accent.unwrap_or_default(),
        has_accent: accent.is_some(),
        is_empty: id.is_none(),
        description: info.as_ref().and_then(|i| i.description()).unwrap_or_default().into(),
        addable: false,
    }
}

/// Everything the Slint navicust section needs, baked once per
/// selection change (tango's `build_navicust_render` + parts panel,
/// selection.rs / save_view/navicust).
pub struct NavicustModel {
    /// The composed grid image (background + color bar + body), baked
    /// at 2× its display size so it stays crisp.
    pub image: slint::Image,
    /// Width / height, so the layout can size the image box off its
    /// height and keep narrower grids proportionally smaller.
    pub aspect: f32,
    /// BN3 style name ("" for the other games) — overlaid on the color
    /// bar by the Slint layer, which gets script-aware font fallback
    /// for free (the original rasterized it through cosmic-text).
    pub style_name: String,
    /// The label line's position in the image's own coordinate space.
    pub label_x_frac: f32,
    pub label_y_frac: f32,
    pub label_h_frac: f32,
    /// Installed parts: solid parts first, then plus parts, keeping
    /// slot order within each group (the parts panel's ordering).
    pub parts: Vec<NcpPartRow>,
}

/// Bake the navicust section, or `None` when the save has no navicust
/// view (the tab is gated off).
pub fn navicust_model(loaded: &Loaded) -> Option<NavicustModel> {
    let v = loaded.save.view_navicust()?;
    let assets = loaded.assets.as_ref();
    let layout = assets.navicust_layout()?;
    let materialized = v.materialized();
    let model = crate::navicust::build_model(&materialized, &layout, v.as_ref(), assets);
    let g = crate::navicust::geometry(model.cols, model.rows);

    // Cell size pinned to the widest (7-col) grid at ~440 display px —
    // baked at 2× so downscales stay crisp — like tango's viewer.
    let ref_g = crate::navicust::geometry(crate::navicust::REFERENCE_COLS, crate::navicust::REFERENCE_COLS);
    let target_w = (g.total_w * (440.0 / ref_g.total_w) * 2.0).round() as u32;
    let image = slint_image(crate::navicust::render(&model, Some(target_w)));

    let style_name = v
        .style()
        .and_then(|sid| assets.style(sid).and_then(|s| s.name()))
        .unwrap_or_default();
    // The BN3 label sits on the color bar's left edge (see grid.rs
    // `render` in the tango crate): x past the padding + border, the
    // bar spanning [PADDING_V, PADDING_V + bar_h] vertically.
    let label_x_frac =
        (crate::navicust::PADDING_H as f32 + crate::navicust::BORDER_WIDTH + 4.0) / g.total_w;
    let label_y_frac = crate::navicust::PADDING_V as f32 / g.total_h;
    let label_h_frac = g.bar_h / g.total_h;

    let mut solid = Vec::new();
    let mut plus = Vec::new();
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let Some(color) = info.color() else { continue };
        let name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        // Thumb baked at the part's installed rotation + compression,
        // so it matches how the part sits in the grid.
        let bitmap = info
            .compressed_bitmap()
            .filter(|_| part.compressed)
            .unwrap_or_else(|| info.uncompressed_bitmap());
        let rotated = crate::navicust::rotate_bitmap(&bitmap, part.rot);
        let (solid_c, plus_c) = crate::navicust::part_colors(color.clone());
        let thumb = crate::navicust::render_part_thumb(&rotated, color, info.is_solid());
        let c = if info.is_solid() { solid_c } else { plus_c };
        let row = NcpPartRow {
            thumb: thumb.map(slint_image).unwrap_or_default(),
            name: name.into(),
            tint: slint::Color::from_rgb_u8(c[0], c[1], c[2]),
            desc: info.description().unwrap_or_default().into(),
        };
        if info.is_solid() {
            solid.push(row);
        } else {
            plus.push(row);
        }
    }
    solid.extend(plus);

    Some(NavicustModel {
        image,
        aspect: g.total_w / g.total_h,
        style_name,
        label_x_frac,
        label_y_frac,
        label_h_frac,
        parts: solid,
    })
}

/// The NaviCust grid rendered at full native resolution for "copy as
/// image". The BN3 style label is not baked in (the app has no text
/// rasterizer; the on-screen label is a Slint overlay).
pub fn navicust_clipboard_image(loaded: &Loaded) -> Option<image::RgbaImage> {
    let v = loaded.save.view_navicust()?;
    let assets = loaded.assets.as_ref();
    let layout = assets.navicust_layout()?;
    let materialized = v.materialized();
    let model = crate::navicust::build_model(&materialized, &layout, v.as_ref(), assets);
    Some(crate::navicust::render(&model, None))
}

/// The Patch Cards section as flattened display lines, plus which
/// chrome to render (0 = BN5/BN6 badge columns, 1 = BN4 slots).
/// `None` when the save has no patch-cards view.
pub fn patch_card_lines(
    lang: &unic_langid::LanguageIdentifier,
    loaded: &Loaded,
) -> Option<(i32, Vec<PatchCardLine>)> {
    let view = loaded.save.view_patch_cards()?;
    let assets = loaded.assets.as_ref();
    let mut lines = Vec::new();
    match view {
        tango_dataview::save::PatchCardsView::PatchCard56s(v) => {
            for i in 0..v.count() {
                let Some(card) = v.patch_card(i) else { continue };
                let info = assets.patch_card56(card.id);
                let name = info
                    .as_ref()
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| format!("#{}", card.id));
                let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
                let effects = info.map(|c| c.effects()).unwrap_or_default();
                let abilities: Vec<_> = effects.iter().filter(|e| e.is_ability).collect();
                let bugs: Vec<_> = effects.iter().filter(|e| !e.is_ability).collect();
                // A card spans one line per effect badge; the first
                // line carries the index / name / MB cell.
                let effect_name =
                    |e: &&tango_dataview::rom::PatchCard56Effect| e.name.clone().unwrap_or_else(|| "???".to_string());
                for j in 0..abilities.len().max(bugs.len()).max(1) {
                    let ability = abilities.get(j);
                    let bug = bugs.get(j);
                    lines.push(PatchCardLine {
                        first: j == 0,
                        zebra: i as i32,
                        idx: if j == 0 { format!("{:>2}", i + 1) } else { String::new() }.into(),
                        name: if j == 0 { name.clone() } else { String::new() }.into(),
                        mb: if j == 0 { format!("{mb}MB") } else { String::new() }.into(),
                        enabled: card.enabled,
                        ability: ability.map(effect_name).unwrap_or_default().into(),
                        ability_debuff: ability.map(|e| e.is_debuff).unwrap_or(false),
                        bug: bug.map(effect_name).unwrap_or_default().into(),
                        bug_debuff: bug.map(|e| e.is_debuff).unwrap_or(false),
                        bug_plain: false,
                    });
                }
            }
            Some((0, lines))
        }
        tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
            for (slot, slot_label) in PATCH_CARD4_SLOT_LABELS.iter().enumerate() {
                match v.patch_card(slot) {
                    Some(card) => {
                        let info = assets.patch_card4(card.id);
                        let name = info
                            .as_ref()
                            .and_then(|i| i.name())
                            .unwrap_or_else(|| format!("#{}", card.id));
                        // 3-digit catalog number, then "name — effect"
                        // (several cards share a name within a slot;
                        // the effect tells them apart).
                        let label = match info.as_ref().map(|i| i.effect()) {
                            Some(effect) => {
                                format!("{:03} {name} — {}", card.id, patch_card4_effect_label(effect))
                            }
                            None => format!("{:03} {name}", card.id),
                        };
                        lines.push(PatchCardLine {
                            first: true,
                            zebra: slot as i32,
                            idx: (*slot_label).into(),
                            name: label.into(),
                            mb: "".into(),
                            enabled: card.enabled,
                            ability: "".into(),
                            ability_debuff: false,
                            bug: "".into(),
                            bug_debuff: false,
                            bug_plain: false,
                        });
                        // The card's downside as its own purple line —
                        // the effect is in the label, the bug is what
                        // the user should still see at a glance.
                        if let Some(bug) = info.as_ref().and_then(|i| patch_card4_bugs_label(i.bugs())) {
                            lines.push(PatchCardLine {
                                first: false,
                                zebra: slot as i32,
                                idx: "".into(),
                                name: "".into(),
                                mb: "".into(),
                                enabled: card.enabled,
                                ability: "".into(),
                                ability_debuff: false,
                                bug: bug.into(),
                                bug_debuff: true,
                                bug_plain: true,
                            });
                        }
                    }
                    None => lines.push(PatchCardLine {
                        first: true,
                        zebra: slot as i32,
                        idx: (*slot_label).into(),
                        name: crate::t!(lang, "patch-card4-none").into(),
                        mb: "".into(),
                        enabled: false,
                        ability: "".into(),
                        ability_debuff: false,
                        bug: "".into(),
                        bug_debuff: false,
                        bug_plain: false,
                    }),
                }
            }
            Some((1, lines))
        }
    }
}

/// BN4 catalog-slot labels (the "0A"–"0F" the game shows).
const PATCH_CARD4_SLOT_LABELS: [&str; 6] = ["0A", "0B", "0C", "0D", "0E", "0F"];

/// Human-readable label for a BN4 patch-card effect (tango's
/// save_view/patch_cards.rs; B-shortcut chip params shown raw — the
/// shortcut → chip-id table isn't mapped).
fn patch_card4_effect_label(effect: tango_dataview::rom::PatchCard4Effect) -> String {
    use tango_dataview::rom::{
        PatchCard4Aura as A, PatchCard4Color as C, PatchCard4Effect as E, PatchCard4Panel as P,
        PatchCard4PetColor as PT, PatchCard4Soul as S,
    };
    match effect {
        E::None => "—".to_string(),
        E::PetMenu(c) => format!(
            "{} PET menu",
            match c {
                PT::Blue => "Blue",
                PT::Pink => "Pink",
                PT::Green => "Green",
                PT::Black => "Black",
            }
        ),
        E::MaxHp(n) => format!("Max HP +{n}"),
        E::BusterAttack(n) => format!("Buster Attack {}", n as u16 + 1),
        E::BButton(s) => format!("B Button {s:?}"),
        E::BCharge(s) => format!("B Charge {s:?}"),
        E::BLeft(s) => format!("B + ← {s:?}"),
        E::CustomSlots(n) => format!("Custom +{n}"),
        E::MegaFolder(n) => format!("Mega Chip +{n}"),
        E::GigaFolder(n) => format!("Giga Chip +{n}"),
        E::TripleSupporter => "Triple Supporter".to_string(),
        E::PanelStep(p) => format!(
            "{} Panel Step",
            match p {
                P::Broken => "Broken",
                P::Cracked => "Cracked",
                P::Metal => "Metal",
                P::Holy => "Holy",
            }
        ),
        E::FullSynchro => "Full Synchro".to_string(),
        E::Aura(a) => match a {
            A::Barrier100 => "Barrier 100",
            A::Barrier200 => "Barrier 200",
            A::LifeAura => "LifeAura",
        }
        .to_string(),
        E::Soul(s) => format!(
            "{} Soul",
            match s {
                S::Roll => "Roll",
                S::Guts => "Guts",
                S::Wind => "Wind",
                S::Search => "Search",
                S::Fire => "Fire",
                S::Thunder => "Thunder",
                S::Proto => "Proto",
                S::Number => "Number",
                S::Metal => "Metal",
                S::Junk => "Junk",
                S::Aqua => "Aqua",
                S::Wood => "Wood",
            }
        ),
        E::Color(c) => format!(
            "{} MegaMan",
            match c {
                C::Red => "Red",
                C::Yellow => "Yellow",
                C::White => "White",
                C::Green => "Green",
            }
        ),
        E::AllGuard => "All Guard".to_string(),
    }
}

/// Joined human-readable label for a BN4 card's bugs, or `None`.
fn patch_card4_bugs_label(bugs: &[tango_dataview::rom::PatchCard4Bug]) -> Option<String> {
    use tango_dataview::rom::PatchCard4Bug as B;
    if bugs.is_empty() {
        return None;
    }
    Some(
        bugs.iter()
            .map(|b| match b {
                B::Confused => "Start battle Confused",
                B::AutoMove => "Auto-move forward",
                B::Hp(_) => "HP Bug",
                B::CustomHP => "Custom HP Bug",
                B::CustomMinus1 => "Custom −1",
                B::PoisonPanelStep => "Poison Panel Step",
            })
            .collect::<Vec<_>>()
            .join(" & "),
    )
}

/// The Auto Battle Data section as flattened rows: six titled sections
/// (grouped runs, so a chip filling several deck slots reads as one
/// "N× chip" row; unfilled runs render as "—" rows). Empty when the
/// save has no ABD view.
pub fn abd_rows(lang: &unic_langid::LanguageIdentifier, loaded: &Loaded) -> Vec<AbdRow> {
    let Some(view) = loaded.save.view_auto_battle_data() else {
        return Vec::new();
    };
    let assets = loaded.assets.as_ref();
    let grouped = tango_dataview::auto_battle_data::GroupedAutoBattleData::materialize(view.as_ref(), assets);
    let sections: [(String, &Vec<(Option<usize>, usize)>); 6] = [
        (
            crate::t!(lang, "auto-battle-data-secondary-standard-chips"),
            &grouped.secondary_standard_chips,
        ),
        (crate::t!(lang, "auto-battle-data-standard-chips"), &grouped.standard_chips),
        (crate::t!(lang, "auto-battle-data-mega-chips"), &grouped.mega_chips),
        (crate::t!(lang, "auto-battle-data-giga-chip"), &grouped.giga_chip),
        (crate::t!(lang, "auto-battle-data-combos"), &grouped.combos),
        (
            crate::t!(lang, "auto-battle-data-program-advance"),
            &grouped.program_advance,
        ),
    ];
    let mut rows = Vec::new();
    for (title, runs) in sections {
        rows.push(AbdRow {
            is_header: true,
            title: title.into(),
            chip: ChipRow::default(),
            zebra: 0,
        });
        for (zebra, (id, count)) in runs.iter().enumerate() {
            let g = GroupedChip {
                count: *count,
                ..GroupedChip::default()
            };
            rows.push(AbdRow {
                is_header: false,
                title: Default::default(),
                chip: make_chip_row(loaded, *id, None, &g),
                zebra: zebra as i32,
            });
        }
    }
    rows
}

/// The folder editor's chip library: one row per (chip, code) the
/// player owns in the pack (pack count > 0), id order, filtered by a
/// case-insensitive name substring. Returns the display rows plus the
/// parallel `(chip_id, code)` values the add callback consumes; each
/// row's `addable` reflects the folder-full/copy-cap/class-cap checks
/// (tango's sorted_library_entries + per-row gating).
pub fn folder_library(loaded: &Loaded, filter: &str) -> (Vec<ChipRow>, Vec<(usize, tango_dataview::save::ChipCode)>) {
    use tango_dataview::save::ChipCode;
    let assets = loaded.assets.as_ref();
    let chips_view = loaded.save.view_chips();
    let folder_idx = chips_view.as_ref().map(|v| v.equipped_folder_index()).unwrap_or(0);
    let folder_full = chips_view
        .as_ref()
        .map(|v| (0..MAX_FOLDER_CHIPS).all(|i| v.chip(folder_idx, i).is_some()))
        .unwrap_or(true);
    let limits = loaded
        .save
        .view_navi()
        .map(|nv| nv.folder_limits(assets))
        .unwrap_or_default();
    let usage = crate::save_edit::FolderUsage::scan(loaded, folder_idx);
    let filter = filter.to_lowercase();

    let mut rows = Vec::new();
    let mut values = Vec::new();
    for id in 0..assets.num_chips() {
        let Some(info) = assets.chip(id) else { continue };
        let Some(name) = info.name() else { continue };
        if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
            continue;
        }
        let addable = !folder_full && usage.can_add(loaded, id, &limits);
        for (variant, ch) in info.codes().into_iter().enumerate() {
            let Some(code) = ChipCode::from_char(ch) else { continue };
            let owned = chips_view
                .as_ref()
                .and_then(|v| v.pack_count(id, variant))
                .is_some_and(|c| c > 0);
            if !owned {
                continue;
            }
            let mut row = make_chip_row(loaded, Some(id), Some(code.to_string()), &GroupedChip::default());
            row.addable = addable;
            rows.push(row);
            values.push((id, code));
        }
    }
    (rows, values)
}

/// The patch-card editor's library: unregistered cards for BN5/BN6
/// (addable while the list has room and the 80MB budget fits), or the
/// full slot catalog for BN4 (labelled by slot, always addable — an
/// add replaces its own slot). Rows reuse ChipRow (icon empty); the
/// parallel values are card ids.
pub fn patch_card_library(loaded: &Loaded, filter: &str) -> (Vec<ChipRow>, Vec<usize>) {
    let assets = loaded.assets.as_ref();
    let filter = filter.to_lowercase();
    let mut rows = Vec::new();
    let mut values = Vec::new();
    match loaded.save.view_patch_cards() {
        Some(tango_dataview::save::PatchCardsView::PatchCard56s(v)) => {
            let registered: std::collections::HashSet<usize> =
                (0..v.count()).filter_map(|i| v.patch_card(i).map(|c| c.id)).collect();
            let enabled_mb: u32 = (0..v.count())
                .filter_map(|i| v.patch_card(i))
                .filter(|c| c.enabled)
                .filter_map(|c| assets.patch_card56(c.id).map(|i| i.mb() as u32))
                .sum();
            let full = v.count() >= assets.num_patch_card56s();
            for id in 0..assets.num_patch_card56s() {
                if registered.contains(&id) {
                    continue;
                }
                let Some(info) = assets.patch_card56(id) else { continue };
                let Some(name) = info.name() else { continue };
                if name.trim().is_empty() {
                    continue;
                }
                if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                    continue;
                }
                let mb = info.mb();
                let mut row = make_chip_row(loaded, None, None, &GroupedChip::default());
                row.name = name.into();
                row.mb = format!("{mb}MB").into();
                row.is_empty = false;
                row.addable = !full && enabled_mb + mb as u32 <= crate::save_edit::MAX_PATCH_CARD56_MB;
                rows.push(row);
                values.push(id);
            }
        }
        Some(tango_dataview::save::PatchCardsView::PatchCard4s(_)) => {
            const SLOTS: [&str; 6] = ["0A", "0B", "0C", "0D", "0E", "0F"];
            for id in 0..assets.num_patch_card4s() {
                let Some(info) = assets.patch_card4(id) else { continue };
                let Some(name) = info.name() else { continue };
                if name.trim().is_empty() {
                    continue;
                }
                if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                    continue;
                }
                let slot = info.slot() as usize;
                let mut row = make_chip_row(loaded, None, None, &GroupedChip::default());
                row.name = format!(
                    "{} {:03} {name}",
                    SLOTS.get(slot).copied().unwrap_or("??"),
                    id
                )
                .into();
                row.is_empty = false;
                row.addable = true;
                rows.push(row);
                values.push(id);
            }
        }
        None => {}
    }
    (rows, values)
}

/// The auto-battle-data editor's library: program advances (always
/// offered) plus every pack-owned Standard/Mega/Giga chip, id order,
/// filtered by name (tango's sorted_auto_battle_data_chips). Returns
/// display rows + parallel chip ids.
pub fn abd_library(loaded: &Loaded, filter: &str) -> (Vec<crate::AbdLibRow>, Vec<usize>) {
    use tango_dataview::rom::ChipClass as CC;
    let assets = loaded.assets.as_ref();
    let view = loaded.save.view_auto_battle_data();
    let chips_view = loaded.save.view_chips();
    let filter = filter.to_lowercase();
    let mut rows = Vec::new();
    let mut values = Vec::new();
    for id in 0..assets.num_chips() {
        let Some(info) = assets.chip(id) else { continue };
        let class = info.class();
        let is_pa = class == CC::ProgramAdvance;
        if !is_pa && !matches!(class, CC::Standard | CC::Mega | CC::Giga) {
            continue;
        }
        let Some(name) = info.name() else { continue };
        if name.trim().is_empty() {
            continue;
        }
        if !is_pa {
            let in_pack = (0..info.codes().len()).any(|variant| {
                chips_view
                    .as_ref()
                    .and_then(|v| v.pack_count(id, variant))
                    .is_some_and(|c| c > 0)
            });
            if !in_pack {
                continue;
            }
        }
        if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
            continue;
        }
        let used = view.as_ref().and_then(|v| v.chip_use_count(id)).unwrap_or(0);
        let is_standard = matches!(class, CC::Standard);
        let sec = if is_standard {
            view.as_ref().and_then(|v| v.secondary_chip_use_count(id)).unwrap_or(0)
        } else {
            0
        };
        rows.push(crate::AbdLibRow {
            icon: loaded.chip_icons.get(id).cloned().flatten().unwrap_or_default(),
            name: name.into(),
            used: used as i32,
            sec: sec as i32,
            has_sec: is_standard,
        });
        values.push(id);
    }
    (rows, values)
}

/// The navi editor's roster: every navi in the ROM's own display
/// order (tango's render_navi_edit), flattened. Returns display cells
/// + parallel navi ids.
pub fn navi_roster(loaded: &Loaded) -> (Vec<crate::NaviCell>, Vec<usize>) {
    let assets = loaded.assets.as_ref();
    let current = loaded.save.view_navi().map(|nv| nv.navi());
    let mut cells = Vec::new();
    let mut values = Vec::new();
    for order_row in assets.navi_order() {
        for &id in order_row.iter() {
            let name = assets
                .navi(id)
                .and_then(|n| n.name())
                .unwrap_or_else(|| format!("Navi #{id}"));
            cells.push(crate::NaviCell {
                emblem: loaded.navi_emblems.get(&id).cloned().unwrap_or_default(),
                name: name.into(),
                selected: current == Some(id),
            });
            values.push(id);
        }
    }
    (cells, values)
}

// ----- copy-as-text renderings (tango's save_view tab_as_text) -----

/// The active section as TSV text for the clipboard, keyed by the
/// Slint-side section kind (0 = NaviCust, 1 = Folder, 2 = Patch Cards,
/// 3 = Auto Battle Data).
pub fn section_as_text(loaded: &Loaded, kind: i32, folder_grouped: bool) -> Option<String> {
    match kind {
        0 => navicust_as_text(loaded),
        1 => folder_as_text(loaded, folder_grouped),
        2 => patch_cards_as_text(loaded),
        3 => abd_as_text(loaded),
        _ => None,
    }
}

/// Two TSV columns — solid parts | plus parts — lined up row-by-row to
/// match the side-by-side layout, with the BN3 style name first.
fn navicust_as_text(loaded: &Loaded) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let v = loaded.save.view_navicust()?;
    let mut out = String::new();
    if let Some(style_id) = v.style() {
        if let Some(name) = assets.style(style_id).and_then(|s| s.name()) {
            out.push_str(&name);
            out.push('\n');
        }
    }
    let mut solid = Vec::new();
    let mut plus = Vec::new();
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        if info.is_solid() {
            solid.push(name);
        } else {
            plus.push(name);
        }
    }
    for i in 0..solid.len().max(plus.len()) {
        let s = solid.get(i).map(String::as_str).unwrap_or("");
        let p = plus.get(i).map(String::as_str).unwrap_or("");
        out.push_str(s);
        out.push('\t');
        out.push_str(p);
        out.push('\n');
    }
    Some(out)
}

/// The folder as TSV, honoring the grouped toggle (tango's
/// folder::as_text; the [REG]/[TAG] markers ride as a suffix column).
fn folder_as_text(loaded: &Loaded, grouped: bool) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let chips_view = loaded.save.view_chips()?;
    let folder_idx = chips_view.equipped_folder_index();
    let regular_idx = chips_view.regular_chip_index(folder_idx).flatten();
    let tag_idxs = chips_view.tag_chip_indexes(folder_idx).flatten();

    let chips: Vec<Option<tango_dataview::save::Chip>> =
        (0..MAX_FOLDER_CHIPS).map(|i| chips_view.chip(folder_idx, i)).collect();

    let mut out = String::new();
    if grouped {
        // Ordered dedup by chip identity — 30 entries, linear find does.
        let mut groups: Vec<(Option<tango_dataview::save::Chip>, GroupedChip)> = Vec::new();
        for (i, chip) in chips.iter().enumerate() {
            let slot = match groups.iter().position(|(c, _)| c == chip) {
                Some(pos) => pos,
                None => {
                    groups.push((chip.clone(), GroupedChip::default()));
                    groups.len() - 1
                }
            };
            let g = &mut groups[slot].1;
            g.count += 1;
            if regular_idx == Some(i) {
                g.is_regular = true;
            }
            if let Some(t) = tag_idxs {
                g.tag_count += (t[0] == i) as i32 + (t[1] == i) as i32;
            }
        }
        for (chip, g) in &groups {
            let Some(c) = chip else {
                out.push_str(&format!("{}\t---\n", g.count));
                continue;
            };
            let name = assets
                .chip(c.id)
                .and_then(|info| info.name())
                .unwrap_or_else(|| "???".to_string());
            out.push_str(&format!("{}\t{name}\t{}", g.count, c.code));
            let mut suffix = vec![];
            if g.is_regular {
                suffix.push("[REG]");
            }
            suffix.extend(std::iter::repeat_n("[TAG]", g.tag_count.max(0) as usize));
            if !suffix.is_empty() {
                out.push('\t');
                out.push_str(&suffix.join(""));
            }
            out.push('\n');
        }
    } else {
        for (i, chip) in chips.iter().enumerate() {
            let Some(c) = chip else {
                out.push_str("---\n");
                continue;
            };
            let name = assets
                .chip(c.id)
                .and_then(|info| info.name())
                .unwrap_or_else(|| "???".to_string());
            out.push_str(&format!("{name}\t{}", c.code));
            let mut suffix = vec![];
            if regular_idx == Some(i) {
                suffix.push("[REG]");
            }
            if let Some(ti) = tag_idxs {
                if ti.contains(&i) {
                    suffix.push("[TAG]");
                }
            }
            if !suffix.is_empty() {
                out.push('\t');
                out.push_str(&suffix.join(""));
            }
            out.push('\n');
        }
    }
    Some(out)
}

/// The enabled patch cards as TSV (tango's patch_cards::as_text).
fn patch_cards_as_text(loaded: &Loaded) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let view = loaded.save.view_patch_cards()?;
    let mut out = String::new();
    match view {
        tango_dataview::save::PatchCardsView::PatchCard56s(v) => {
            for i in 0..v.count() {
                let Some(card) = v.patch_card(i) else { continue };
                if !card.enabled {
                    continue;
                }
                let info = assets.patch_card56(card.id);
                let name = info
                    .as_ref()
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| format!("#{}", card.id));
                let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
                out.push_str(&format!("{name}\t{mb}MB\n"));
            }
        }
        tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
            for (i, slot_label) in PATCH_CARD4_SLOT_LABELS.iter().enumerate() {
                let Some(card) = v.patch_card(i) else { continue };
                if !card.enabled {
                    continue;
                }
                let info = assets.patch_card4(card.id);
                let name = info
                    .as_ref()
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| format!("#{}", card.id));
                out.push_str(&format!("{slot_label}\t{name}\n"));
            }
        }
    }
    Some(out)
}

/// The materialized ABD deck as sectioned text (tango's abd::as_text —
/// section headers intentionally English, like the original).
fn abd_as_text(loaded: &Loaded) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let view = loaded.save.view_auto_battle_data()?;
    let mat = view.materialized();
    let chip_name = |id: Option<usize>| match id {
        Some(id) => assets
            .chip(id)
            .and_then(|c| c.name())
            .unwrap_or_else(|| format!("#{id}")),
        None => "—".to_string(),
    };
    let mut out = String::new();
    let mut section = |title: &str, ids: &[Option<usize>]| {
        out.push_str(&format!("[{title}]\n"));
        for id in ids {
            out.push_str(&chip_name(*id));
            out.push('\n');
        }
        out.push('\n');
    };
    section("Secondary standard", mat.secondary_standard_chips());
    section("Standard", mat.standard_chips());
    section("Mega", mat.mega_chips());
    section("Giga", &[mat.giga_chip()]);
    section("Combos", mat.combos());
    section("Program advance", &[mat.program_advance()]);
    Some(out)
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
