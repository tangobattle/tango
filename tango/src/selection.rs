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
        save: Box<dyn tango_dataview::save::Save + Send + Sync>,
        patches_path: &std::path::Path,
        patch: Option<(String, semver::Version, Arc<crate::patch::Version>)>,
    ) -> Self {
        // Apply the BPS patch to the raw ROM if one is selected. On
        // failure we fall back to the unpatched ROM (and log) so the
        // save view still renders.
        let (rom, applied_patch) = match patch {
            Some((name, version, meta)) => match crate::patch::apply_patch_from_disk(
                &rom, game, patches_path, &name, &version,
            ) {
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
            },
            None => (rom, None),
        };

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

        Self {
            game,
            save_path,
            save,
            patch: applied_patch,
            assets,
            chip_icons,
            chip_images,
            element_icons,
            navi_emblems,
            navicust_render,
        }
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
        let variant = u8::try_from(gi.rom_variant)
            .map_err(|_| anyhow::anyhow!("variant {} out of range", gi.rom_variant))?;
        let game = tango_gamedb::find_by_family_and_variant(&gi.rom_family, variant)
            .ok_or_else(|| anyhow::anyhow!("unknown rom {}/{}", gi.rom_family, gi.rom_variant))?;
        let rom = scanners
            .roms
            .read()
            .get(&game)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("rom for {}/{} not scanned", gi.rom_family, gi.rom_variant))?;

        let save = game.save_from_wram(&replay.local_wram)?;

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
    //
    // HiDPI strategy: render the source at 2× the visual display
    // width, then tell the iced widget to paint at 1× (display
    // width logical pixels). On a 2x DPI display iced's surface is
    // 2× device pixels per logical pixel, so the 2× source maps
    // 1:1 to device pixels — pixel-perfect cosmic-text rasterization.
    // On a 1x display iced linear-downsamples 2:1; the colored
    // tiles look fine and text is only mildly softer than rendered-
    // at-display-size.
    const PADDING_H: f32 = crate::navicust::PADDING_H as f32;
    const PADDING_V: f32 = crate::navicust::PADDING_V as f32;
    const SQUARE_SIZE: f32 = crate::navicust::SQUARE_SIZE;
    const BORDER_WIDTH: f32 = crate::navicust::BORDER_WIDTH;
    /// Visual width the iced widget paints the navicust at (in
    /// logical pixels). The Handle behind it is 2× this so HiDPI
    /// stays crisp.
    pub const DISPLAY_TARGET_W: u32 = 280;
    const OVERSAMPLE: u32 = 2;
    let (rows, cols) = materialized.dim();

    let lang = crate::game::region_to_language(game.region());
    let img = crate::navicust::render(
        &materialized,
        &layout,
        view.as_ref(),
        assets,
        &lang,
        Some(DISPLAY_TARGET_W * OVERSAMPLE),
    );
    let (handle_w, handle_h) = (img.width(), img.height());

    // Geometry in DISPLAY (logical) coords — the overlay sits on
    // top of the iced widget at its 1× size, so divide by
    // OVERSAMPLE to get back to display units. `display_scale`
    // accounts for grids that fit under the cap natively
    // (scale = 1.0 case there).
    let body_w_native = cols as f32 * SQUARE_SIZE + BORDER_WIDTH;
    let total_w_native = body_w_native + PADDING_H * 2.0;
    let display_w = handle_w as f32 / OVERSAMPLE as f32;
    let display_h = handle_h as f32 / OVERSAMPLE as f32;
    let display_scale = display_w / total_w_native;
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

fn cropped_handle(src: &image::RgbaImage, x: u32, y: u32, w: u32, h: u32) -> iced_image::Handle {
    let sub = image::imageops::crop_imm(src, x, y, w, h).to_image();
    iced_image::Handle::from_rgba(w, h, sub.into_raw())
}
