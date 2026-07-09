use crate::rom::GameRef;
use crate::rom_overrides::OverridenAssets;
use iced::widget::image as iced_image;
use std::collections::HashMap;
use std::sync::Arc;

/// Currently committed game + save + their derived ROM/assets +
/// preloaded icon image handles.
///
/// Assets are derived from the ROM and the save's WRAM; image handles
/// are derived from assets. All of this is rebuilt only when game or
/// save changes, so per-frame `view()` stays cheap.
/// The currently committed patch (name + version + arc to the per-version
/// metadata). Held alongside the loaded ROM so refresh decisions know
/// whether the active selection still matches.
#[derive(Clone)]
pub struct AppliedPatch {
    pub name: String,
    pub version: semver::Version,
    pub version_meta: Arc<crate::patch::Version>,
}

/// Which sections of a loaded save can be edited in place. Each flag is a
/// pure capability probe — `view_*_mut().is_some()` — which needs `&mut save`,
/// so it's computed once and cached on the immutable `Loaded` (the per-frame
/// view only holds `&Loaded`, and the read-only `view_*()` probes answer a
/// different question: BN3 has a viewable-but-not-writable navicust, BN1–4 a
/// viewable-but-not-writable navi). Swapping the equipped navi flips some of
/// these (a link navi has no navicust / patch cards), so re-probe via
/// [`Loaded::refresh_editability`] after any in-memory edit that can change
/// capability.
#[derive(Clone, Copy, Default)]
pub struct Editability {
    /// `view_chips_mut().is_some()` — drives the Folder tab's Edit button.
    pub folder: bool,
    /// `view_navicust_mut().is_some()` (BN4/5/6, and not a link navi).
    pub navicust: bool,
    /// `view_navi_mut().is_some()` — the equipped navi (BN5/BN6/BN4.5),
    /// or BN4's EXE4.5-link-assignable operate navi.
    pub navi: bool,
    /// `view_patch_cards_mut().is_some()` — BN4 (PatchCard4s, slot-based) and
    /// BN5/BN6 (PatchCard56s, list-based); each gets its own editor.
    pub patch_cards: bool,
    /// `view_auto_battle_data_mut().is_some()` (BN4/BN5).
    pub auto_battle_data: bool,
}

impl Editability {
    /// Probe every section's writable view once. Constructing a mutable view
    /// has no side effects, so this is a pure capability check.
    fn probe(save: &mut (dyn tango_dataview::save::Save + Send + Sync)) -> Self {
        // Each `is_some()` gets its own statement so the borrowed view temporary
        // is dropped before the next probe — a single struct literal would keep
        // every mutable borrow of `save` alive at once.
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

    /// Whether *any* section is editable — drives the single save-level Edit
    /// button (once open, the user navigates tabs to edit each section).
    pub fn any(&self) -> bool {
        self.folder || self.navicust || self.navi || self.patch_cards || self.auto_battle_data
    }
}

pub struct Loaded {
    pub game: GameRef,
    pub save_path: std::path::PathBuf,
    pub save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    /// Which sections of this save can be edited in place. See [`Editability`].
    pub editability: Editability,
    /// Patch+version baked into this Loaded, if any. `None` = raw ROM.
    pub patch: Option<AppliedPatch>,
    pub assets: Box<dyn tango_dataview::rom::Assets + Send + Sync>,
    pub chip_icons: Vec<Option<iced_image::Handle>>,
    /// Full-size chip images (variable dimensions) for hover previews.
    pub chip_images: Vec<Option<(u32, u32, iced_image::Handle)>>,
    pub element_icons: HashMap<usize, iced_image::Handle>,
    pub navi_emblems: HashMap<usize, iced_image::Handle>,
    /// Signature color per navi, extracted from its emblem pixels —
    /// drives the Link Navi card's plate/glow tint. Missing when the
    /// emblem is entirely monochrome (then the card falls back to a
    /// neutral accent).
    pub navi_accents: HashMap<usize, iced::Color>,
    /// Precomputed NaviCust grid image, from `view_navicust()`. None for
    /// link navis (no navicust) or when no navicust_layout is published.
    pub navicust_render: Option<NavicustRender>,
    /// Per-part shape thumbnails (compressed footprint, in the part's
    /// color) for the navicust editor palette, as `(width, height,
    /// handle)`. Indexed by part id; `None` = no shape / no color. Baked
    /// once here so the per-frame palette just clones handles.
    pub navicust_part_icons: Vec<Option<(u32, u32, iced_image::Handle)>>,
    /// Cropped shape thumbnails, one per *installed navicust slot*, baked at
    /// that slot's actual rotation + compression, so the read-only Navi
    /// tab's inline parts list shows each part as it sits in the grid rather
    /// than its default footprint. Trimmed to the shape's bounding box (the
    /// grid-sized transparent margin the palette wants would just push the
    /// name text away). Indexed by navicust slot; `None` for an empty slot
    /// or a part with no color / shape. Empty for saves without a navicust.
    /// Rebuilt by [`Loaded::rebuild_navicust_render`].
    pub navicust_installed_part_thumbs: Vec<Option<(u32, u32, iced_image::Handle)>>,
    /// Logos for the Cover tab, as `(width, height, handle)`. The
    /// loaded game's own variant comes first; any sibling variants in
    /// the family follow (so families with two logos — Gregar/Falzar
    /// etc. — can fan both out). Empty when the game has no per-game
    /// `Game` registration. Built once here so the per-frame view()
    /// just clones the handles.
    pub logos: Vec<(u32, u32, iced_image::Handle)>,
}

/// Cached NaviCust image plus everything needed to translate a pointer
/// position over the displayed image back to a part index (for hover
/// highlighting in the parts list).
pub struct NavicustRender {
    pub source_w: u32,
    pub source_h: u32,
    pub handle: iced_image::Handle,
    /// Top-left of the cell grid in source-image coordinates.
    pub body_origin_x: f32,
    pub body_origin_y: f32,
    /// Edge length of one cell in source-image coordinates.
    pub cell_size: f32,
    pub cols: usize,
    pub rows: usize,
    /// Flat row-major materialized grid; `None` = empty cell, `Some(i)`
    /// = `navicust_part(i)` index.
    pub cell_part_idx: Vec<Option<usize>>,
}

impl Loaded {
    /// Build from a *raw* (unpatched) ROM, applying the selected patch
    /// from disk first. On apply failure we fall back to the unpatched
    /// ROM (and log) so the save view still renders. Callers that
    /// already hold the patched image should use [`from_patched_rom`]
    /// instead, to avoid applying the patch a second time.
    ///
    /// [`from_patched_rom`]: Self::from_patched_rom
    pub fn build(
        game: GameRef,
        rom: Vec<u8>,
        save_path: std::path::PathBuf,
        save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        patches_path: &std::path::Path,
        patch: Option<(String, semver::Version, Arc<crate::patch::Version>)>,
    ) -> Self {
        let (rom, applied_patch) = match patch {
            Some((name, version, meta)) => {
                match crate::patch::apply_patch_from_disk(&rom, game, patches_path, &name, &version) {
                    Ok(patched) => (
                        patched,
                        Some(AppliedPatch {
                            name,
                            version,
                            version_meta: meta,
                        }),
                    ),
                    Err(e) => {
                        log::error!(
                            "failed to apply patch {name} v{version} to {:?}: {e}",
                            game.family_and_variant()
                        );
                        (rom, None)
                    }
                }
            }
            None => (rom, None),
        };
        Self::from_patched_rom(game, rom, save_path, save, applied_patch)
    }

    /// Build from a ROM that's *already* had its patch applied, plus the
    /// [`AppliedPatch`] that produced it (`None` for a raw ROM). Unlike
    /// [`build`], this never touches the BPS patch — use it when the
    /// caller already holds the patched image (e.g. a live session that
    /// patched the ROM for the emulator) so the patch isn't re-applied
    /// just to read the asset overrides + charset off `applied_patch`.
    ///
    /// [`build`]: Self::build
    pub fn from_patched_rom(
        game: GameRef,
        rom: Vec<u8>,
        save_path: std::path::PathBuf,
        mut save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        applied_patch: Option<AppliedPatch>,
    ) -> Self {
        // Probe section editability once (each needs `&mut save`, but the
        // per-frame view only holds `&Loaded`). Constructing a mutable view has
        // no side effects, so this is a pure capability check we can cache.
        let editability = Editability::probe(&mut *save);

        let wram = save.as_raw_wram().into_owned();
        let charset_owned: Option<Vec<&str>> = applied_patch
            .as_ref()
            .and_then(|p| p.version_meta.rom_overrides.charset.as_ref())
            .map(|c| c.iter().map(|s| s.as_str()).collect());
        let inner = game.load_rom_assets(&rom, &wram, charset_owned.as_deref());
        let overrides = applied_patch
            .as_ref()
            .map(|p| p.version_meta.rom_overrides.clone())
            .unwrap_or_default();
        let assets: Box<dyn tango_dataview::rom::Assets + Send + Sync> =
            Box::new(OverridenAssets::new(inner, overrides));

        // Chip icons (14x14 cropped from 16x16) + full chip images for
        // hover previews. Both lazy per id; pre-pass once at load time
        // so the per-frame view() stays cheap.
        let mut chip_icons: Vec<Option<iced_image::Handle>> = Vec::with_capacity(assets.num_chips());
        let mut chip_images: Vec<Option<(u32, u32, iced_image::Handle)>> = Vec::with_capacity(assets.num_chips());
        for id in 0..assets.num_chips() {
            let chip = assets.chip(id);
            chip_icons.push(chip.as_ref().map(|c| cropped_handle(&c.icon(), 1, 1, 14, 14)));
            chip_images.push(chip.as_ref().map(|c| {
                let img = c.image();
                let (w, h) = (img.width(), img.height());
                (w, h, iced_image::Handle::from_rgba(w, h, img.into_raw()))
            }));
        }

        // Element icons: element ids are sparse; try the small id space.
        let mut element_icons = HashMap::new();
        for id in 0..32 {
            if let Some(img) = assets.element_icon(id) {
                element_icons.insert(id, cropped_handle(&img, 1, 1, 14, 14));
            }
        }

        // Navi emblems for LinkNavi games: 15x15 from (1,0). The accent
        // color (most prominent saturated pixel color) is pulled from the
        // same crop for the Link Navi card's tinting. Games whose emblems
        // aren't extractable (BN4) just leave the maps empty — the navi
        // card and editor already render name-only when an id is missing.
        let mut navi_emblems = HashMap::new();
        let mut navi_accents = HashMap::new();
        for id in 0..assets.num_navis() {
            let Some(emblem) = assets.navi(id).and_then(|navi| navi.emblem()) else {
                continue;
            };
            let crop = image::imageops::crop_imm(&emblem, 1, 0, 15, 15).to_image();
            if let Some(accent) = emblem_accent(&crop) {
                navi_accents.insert(id, accent);
            }
            let (w, h) = crop.dimensions();
            navi_emblems.insert(id, iced_image::Handle::from_rgba(w, h, crop.into_raw()));
        }

        // Render the NaviCust grid once per save+game.
        let navicust_render = build_navicust_render(save.as_ref(), assets.as_ref(), game);

        // Bake the grid-sized shape thumbnail per navicust part for the
        // editor palette (aligned blocks). The read-only viewer's inline
        // parts list instead uses per-slot crops baked at each part's actual
        // orientation (see `navicust_installed_part_thumbs` below).
        let mut navicust_part_icons: Vec<Option<(u32, u32, iced_image::Handle)>> =
            Vec::with_capacity(assets.num_navicust_parts());
        for id in 0..assets.num_navicust_parts() {
            let img = assets.navicust_part(id).and_then(|info| {
                let color = info.color()?;
                crate::save_view::navicust::grid::render_part_thumb(
                    &info.compressed_bitmap().unwrap_or_else(|| info.uncompressed_bitmap()),
                    color,
                    info.is_solid(),
                    false,
                )
            });
            navicust_part_icons.push(img.map(|img| {
                let (w, h) = (img.width(), img.height());
                (w, h, iced_image::Handle::from_rgba(w, h, img.into_raw()))
            }));
        }
        let navicust_installed_part_thumbs = build_navicust_part_thumbs(save.as_ref(), assets.as_ref());

        // Logos for the Cover tab. The loaded variant goes first; its
        // family siblings (the other color version, where one exists)
        // follow so the Cover tab can fan both out. The per-game
        // `LazyImage` caches the PNG decode; `to_rgba8` + `from_rgba`
        // run once here so the per-frame view() just clones handles.
        let (family, variant) = game.family_and_variant();
        let mut logo_order: Vec<GameRef> = vec![game];
        for g in crate::game::games_in_family(family) {
            if g.family_and_variant().1 != variant {
                logo_order.push(g);
            }
        }
        let logos: Vec<(u32, u32, iced_image::Handle)> = logo_order
            .into_iter()
            .filter_map(|g| crate::game::from_gamedb_entry(g))
            .map(|gi| {
                let img = gi.logo_image.to_rgba8();
                let (w, h) = img.dimensions();
                (w, h, iced_image::Handle::from_rgba(w, h, img.into_raw()))
            })
            .collect();

        Self {
            game,
            save_path,
            save,
            editability,
            patch: applied_patch,
            assets,
            chip_icons,
            chip_images,
            element_icons,
            navi_emblems,
            navi_accents,
            navicust_render,
            navicust_part_icons,
            navicust_installed_part_thumbs,
            logos,
        }
    }

    /// Recompute the baked NaviCust grid image — and the per-slot parts-list
    /// thumbnails — from the current in-memory save. The navicust editor
    /// commits edits into `self.save` (and rebuilds the materialized WRAM
    /// cache) without triggering a full `Loaded` rebuild, so these cached
    /// images would otherwise stay stale until the next reselection.
    pub fn rebuild_navicust_render(&mut self) {
        self.navicust_render = build_navicust_render(self.save.as_ref(), self.assets.as_ref(), self.game);
        self.navicust_installed_part_thumbs = build_navicust_part_thumbs(self.save.as_ref(), self.assets.as_ref());
    }

    /// Re-probe section [`Editability`] from the current in-memory save.
    /// Swapping the equipped navi flips navicust / patch-card capability, so
    /// the edit path calls this after a navi change to keep the cached flags
    /// in sync (they're read from the `&Loaded`-only per-frame view).
    pub fn refresh_editability(&mut self) {
        self.editability = Editability::probe(&mut *self.save);
    }

    /// Build a Loaded for the local side of a replay — used by the
    /// replays tab to embed the save view in its detail panel. Pulls
    /// the local rom + patch from the scanners cache; returns Err
    /// if anything's missing.
    pub fn for_replay_local(
        scanners: &crate::app::Scanners,
        config: &crate::config::Config,
        replay: &tango_pvp::replay::Replay,
    ) -> anyhow::Result<Self> {
        let side = replay
            .metadata
            .local_side
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("replay missing local side metadata"))?;
        let gi = side
            .game_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("replay side has no game info"))?;
        let variant =
            u8::try_from(gi.rom_variant).map_err(|_| anyhow::anyhow!("variant {} out of range", gi.rom_variant))?;
        let game = crate::game::find_by_family_and_variant(&gi.rom_family, variant)
            .ok_or_else(|| anyhow::anyhow!("unknown rom {}/{}", gi.rom_family, gi.rom_variant))?;
        let rom = scanners
            .roms
            .read()
            .get(&game)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("rom for {}/{} not scanned", gi.rom_family, gi.rom_variant))?;

        let save = game.parse_save(&replay.local_sram)?;

        // Optional patch info — pull the Arc<Version> from the patch
        // scanner so we get the same rom_overrides (charset etc.) as
        // the play tab.
        let patch_meta = gi.patch.as_ref().and_then(|p| {
            let v = semver::Version::parse(&p.version).ok()?;
            let patches = scanners.patches.read();
            let pinfo = patches.get(&p.name)?;
            let vmeta = pinfo.versions.get(&v)?.clone();
            Some((p.name.clone(), v, vmeta))
        });

        Ok(Self::build(
            game,
            rom,
            std::path::PathBuf::new(),
            save,
            &config.patches_path(),
            patch_meta,
        ))
    }
}

fn build_navicust_render(
    save: &(dyn tango_dataview::save::Save + Send + Sync),
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    game: GameRef,
) -> Option<NavicustRender> {
    let view = save.view_navicust()?;
    let layout = assets.navicust_layout()?;
    let materialized = view.materialized();

    // Mirrors the constants inside navicust.rs's tiny-skia render.
    const PADDING_H: f32 = crate::save_view::navicust::grid::PADDING_H as f32;
    const PADDING_V: f32 = crate::save_view::navicust::grid::PADDING_V as f32;
    const SQUARE_SIZE: f32 = crate::save_view::navicust::grid::SQUARE_SIZE;
    const BORDER_WIDTH: f32 = crate::save_view::navicust::grid::BORDER_WIDTH;
    let (rows, cols) = materialized.dim();

    let lang = crate::game::region_to_language(game.region());
    // Render at native resolution; the iced widget paints it at the same
    // display width as the interactive editor, so iced scales the high-res
    // source down — keeping it crisp on HiDPI.
    let mut img = crate::save_view::navicust::grid::render(&materialized, &layout, view.as_ref(), assets, &lang, None);
    let (handle_w, handle_h) = (img.width(), img.height());

    // Constant cell size across all games (the 7×7 cell size), so the image
    // grows/shrinks with the grid instead of every grid being squeezed to one
    // total width. Same scale `EditorGrid` uses, so viewer and editor match.
    let display_scale = crate::save_view::navicust::grid::display_scale(crate::save_view::navicust::editor::DISPLAY_W);
    let display_w = handle_w as f32 * display_scale;
    let display_h = handle_h as f32 * display_scale;
    // Round corners to ~4 display px (the pane's radius).
    mask_rounded_corners(&mut img, (4.0 / display_scale).round().max(1.0) as u32);
    let color_bar_h = (SQUARE_SIZE / 2.0 + BORDER_WIDTH).round();
    let body_origin_x = (PADDING_H + BORDER_WIDTH / 2.0) * display_scale;
    let body_origin_y = (PADDING_V + color_bar_h + PADDING_V + BORDER_WIDTH / 2.0) * display_scale;
    let cell_size = SQUARE_SIZE * display_scale;

    let cell_part_idx: Vec<Option<usize>> = materialized.iter().copied().collect();

    Some(NavicustRender {
        // source_w/h advertise the WIDGET (logical) size, not the
        // Handle's pixel size — the iced widget will paint at
        // these dimensions and iced handles the device-pixel scale.
        source_w: display_w.round() as u32,
        source_h: display_h.round() as u32,
        handle: iced_image::Handle::from_rgba(handle_w, handle_h, img.into_raw()),
        body_origin_x,
        body_origin_y,
        cell_size,
        cols,
        rows,
        cell_part_idx,
    })
}

/// Hard-clip the four corners of `img` to transparency so the iced
/// Image widget renders with rounded corners. The pane plate behind it
/// shows through where alpha drops to 0. Iced's container clip(true)
/// only clips to a rectangle, so the rounding has to live in the
/// pixels.
fn mask_rounded_corners(img: &mut image::RgbaImage, radius: u32) {
    let (w, h) = (img.width(), img.height());
    let r = radius.min(w / 2).min(h / 2);
    if r == 0 {
        return;
    }
    let r_sq = (r as f32) * (r as f32);
    // Iterate just the bounding boxes of the four corner squares.
    let corners = [
        // (x_start, y_start, cx_anchor, cy_anchor)
        (0, 0, r as f32, r as f32),
        (w - r, 0, w as f32 - r as f32, r as f32),
        (0, h - r, r as f32, h as f32 - r as f32),
        (w - r, h - r, w as f32 - r as f32, h as f32 - r as f32),
    ];
    for (x0, y0, cx_anchor, cy_anchor) in corners {
        for y in y0..(y0 + r) {
            for x in x0..(x0 + r) {
                let dx = (x as f32 + 0.5) - cx_anchor;
                let dy = (y as f32 + 0.5) - cy_anchor;
                if dx * dx + dy * dy > r_sq {
                    img.get_pixel_mut(x, y).0[3] = 0;
                }
            }
        }
    }
}

fn cropped_handle(src: &image::RgbaImage, x: u32, y: u32, w: u32, h: u32) -> iced_image::Handle {
    let sub = image::imageops::crop_imm(src, x, y, w, h).to_image();
    iced_image::Handle::from_rgba(w, h, sub.into_raw())
}

/// Bake one cropped shape thumbnail per *installed* navicust slot, at the
/// slot's actual rotation + compression, for the read-only Navi tab's parts
/// list. Mirrors the per-id grid-sized icon bake above (same `render_part_thumb`)
/// but renders straight to the shape's bounding box (`crop = true`) and picks
/// the bitmap (compressed vs uncompressed) and rotation from the placed part
/// instead of the part's default footprint. Indexed by navicust slot; `None`
/// for an empty slot or a part with no color / shape. Empty for saves without
/// a navicust.
fn build_navicust_part_thumbs(
    save: &(dyn tango_dataview::save::Save + Send + Sync),
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
) -> Vec<Option<(u32, u32, iced_image::Handle)>> {
    let Some(v) = save.view_navicust() else {
        return Vec::new();
    };
    (0..v.count())
        .map(|i| {
            let part = v.navicust_part(i)?;
            let info = assets.navicust_part(part.id)?;
            let color = info.color()?;
            let bitmap = info
                .compressed_bitmap()
                .filter(|_| part.compressed)
                .unwrap_or_else(|| info.uncompressed_bitmap());
            let rotated = crate::save_view::navicust::grid::rotate_bitmap(&bitmap, part.rot);
            let img = crate::save_view::navicust::grid::render_part_thumb(&rotated, color, info.is_solid(), true)?;
            let (w, h) = (img.width(), img.height());
            Some((w, h, iced_image::Handle::from_rgba(w, h, img.into_raw())))
        })
        .collect()
}

/// The emblem's signature color: every distinct opaque pixel color is
/// scored by frequency, weighted toward saturated mid-to-bright tones so
/// black outlines and white highlights (numerous but characterless) lose
/// to the emblem's actual identity color. `None` when nothing scores —
/// a fully transparent or pure black/white emblem.
fn emblem_accent(img: &image::RgbaImage) -> Option<iced::Color> {
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
        .map(|(_, rgb)| iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2]))
}
