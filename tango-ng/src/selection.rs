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
    pub navicust_image: Option<(u32, u32, iced_image::Handle)>,
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
        let navicust_image = build_navicust_image(save.as_ref(), assets.as_ref());

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
            navicust_image,
        }
    }
}

fn build_navicust_image(
    save: &(dyn tango_dataview::save::Save + Send + Sync),
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
) -> Option<(u32, u32, iced_image::Handle)> {
    let nv = save.view_navi()?;
    let view = match nv {
        tango_dataview::save::NaviView::Navicust(v) => v,
        _ => return None,
    };
    let layout = assets.navicust_layout()?;
    let materialized = view.materialized();
    let img = crate::navicust::render(&materialized, &layout, view.as_ref(), assets);
    let (w, h) = (img.width(), img.height());
    Some((w, h, iced_image::Handle::from_rgba(w, h, img.into_raw())))
}

fn cropped_handle(src: &image::RgbaImage, x: u32, y: u32, w: u32, h: u32) -> iced_image::Handle {
    let sub = image::imageops::crop_imm(src, x, y, w, h).to_image();
    iced_image::Handle::from_rgba(w, h, sub.into_raw())
}
