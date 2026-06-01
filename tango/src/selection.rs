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

pub struct Loaded {
    pub game: GameRef,
    pub save_path: std::path::PathBuf,
    pub save: Box<dyn tango_dataview::save::Save + Send + Sync>,
    /// Whether this save supports in-place folder editing — i.e.
    /// `save.view_chips_mut().is_some()`. Cached at build time because
    /// the probe needs `&mut save`, but the per-frame view only holds
    /// `&Loaded`. Drives whether the Folder tab shows the Edit button.
    pub chips_editable: bool,
    /// Whether this save supports in-place navicust editing — i.e.
    /// `save.view_navi_mut()` yields the `Navicust` variant (BN4/5/6).
    /// Cached at build time (the probe needs `&mut save`); drives
    /// whether the Navi tab shows the Edit button.
    pub navicust_editable: bool,
    /// Whether this save supports in-place patch-card editing — i.e.
    /// `save.view_patch_cards_mut().is_some()`. True for BN4 (PatchCard4s,
    /// slot-based) and BN5/BN6 (PatchCard56s, list-based); each gets its own
    /// editor. Cached at build time (the probe needs `&mut save`); drives
    /// whether the Patch Cards tab shows the Edit button.
    pub patch_cards_editable: bool,
    /// Whether this save supports in-place auto-battle-data editing — i.e.
    /// `save.view_auto_battle_data_mut().is_some()` (BN4/BN5). Cached at
    /// build time (the probe needs `&mut save`); drives whether the Auto
    /// Battle Data tab shows the Edit button.
    pub auto_battle_data_editable: bool,
    /// Patch+version baked into this Loaded, if any. `None` = raw ROM.
    pub patch: Option<AppliedPatch>,
    pub assets: Box<dyn tango_dataview::rom::Assets + Send + Sync>,
    pub chip_icons: Vec<Option<iced_image::Handle>>,
    /// Full-size chip images (variable dimensions) for hover previews.
    pub chip_images: Vec<Option<(u32, u32, iced_image::Handle)>>,
    pub element_icons: HashMap<usize, iced_image::Handle>,
    pub navi_emblems: HashMap<usize, iced_image::Handle>,
    /// Precomputed NaviCust grid image for the Navicust variant. None
    /// for LinkNavi games or when no navicust_layout is published.
    pub navicust_render: Option<NavicustRender>,
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
    pub fn build(
        game: GameRef,
        rom: Vec<u8>,
        save_path: std::path::PathBuf,
        mut save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        patches_path: &std::path::Path,
        patch: Option<(String, semver::Version, Arc<crate::patch::Version>)>,
    ) -> Self {
        // Apply the BPS patch to the raw ROM if one is selected. On
        // failure we fall back to the unpatched ROM (and log) so the
        // save view still renders.
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

        // Probe folder-editability once (needs `&mut save`); constructing
        // the mutable chip view has no side effects, so this is a pure
        // capability check we can cache on the immutable Loaded.
        let chips_editable = save.view_chips_mut().is_some();
        // Navicust editability: only the `Navicust` navi variant (BN3/4/5/6)
        // is writable. LinkNavi games (BN6 with a link navi equipped) stay
        // off. Same pure-capability probe pattern as `chips_editable`.
        let navicust_editable = matches!(
            save.view_navi_mut(),
            Some(tango_dataview::save::NaviViewMut::Navicust(_))
        );
        // Patch-card editability: both BN4 (PatchCard4s) and BN5/BN6
        // (PatchCard56s) are writable, each through its own editor. Same
        // pure-capability probe pattern as the others.
        let patch_cards_editable = save.view_patch_cards_mut().is_some();
        // Auto-battle-data editability: BN4/BN5 expose a writable view.
        // Same pure-capability probe pattern as the others.
        let auto_battle_data_editable = save.view_auto_battle_data_mut().is_some();

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

        // Navi emblems for LinkNavi games: 15x15 from (1,0).
        let mut navi_emblems = HashMap::new();
        for id in 0..assets.num_navis() {
            if let Some(navi) = assets.navi(id) {
                navi_emblems.insert(id, cropped_handle(&navi.emblem(), 1, 0, 15, 15));
            }
        }

        // Render the NaviCust grid once per save+game.
        let navicust_render = build_navicust_render(save.as_ref(), assets.as_ref(), game);

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
            chips_editable,
            navicust_editable,
            patch_cards_editable,
            auto_battle_data_editable,
            patch: applied_patch,
            assets,
            chip_icons,
            chip_images,
            element_icons,
            navi_emblems,
            navicust_render,
            logos,
        }
    }

    /// Recompute the baked NaviCust grid image from the current
    /// in-memory save. The navicust editor commits edits into
    /// `self.save` (and rebuilds the materialized WRAM cache) without
    /// triggering a full `Loaded` rebuild, so the cached image would
    /// otherwise stay stale until the next reselection.
    pub fn rebuild_navicust_render(&mut self) {
        self.navicust_render = build_navicust_render(self.save.as_ref(), self.assets.as_ref(), self.game);
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
        let game = tango_gamedb::find_by_family_and_variant(&gi.rom_family, variant)
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
    let nv = save.view_navi()?;
    let view = match nv {
        tango_dataview::save::NaviView::Navicust(v) => v,
        _ => return None,
    };
    let layout = assets.navicust_layout()?;
    let materialized = view.materialized();

    // Mirrors the constants inside navicust.rs's tiny-skia render.
    const PADDING_H: f32 = crate::navicust::PADDING_H as f32;
    const PADDING_V: f32 = crate::navicust::PADDING_V as f32;
    const SQUARE_SIZE: f32 = crate::navicust::SQUARE_SIZE;
    const BORDER_WIDTH: f32 = crate::navicust::BORDER_WIDTH;
    let (rows, cols) = materialized.dim();

    let lang = crate::game::region_to_language(game.region());
    // Render at native resolution; the iced widget paints it at the same
    // display width as the interactive editor, so iced scales the high-res
    // source down — keeping it crisp on HiDPI.
    let mut img = crate::navicust::render(&materialized, &layout, view.as_ref(), assets, &lang, None);
    let (handle_w, handle_h) = (img.width(), img.height());

    // Constant cell size across all games (the 7×7 cell size), so the image
    // grows/shrinks with the grid instead of every grid being squeezed to one
    // total width. Same scale `EditorGrid` uses, so viewer and editor match.
    let display_scale = crate::navicust::display_scale(crate::navicust_editor::DISPLAY_W);
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
