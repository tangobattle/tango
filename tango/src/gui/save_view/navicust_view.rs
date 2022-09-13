use fluent_templates::Loader;

use crate::{gui, i18n, rom, save};

pub struct State {
    rendered_navicust_cache: Option<(image::RgbaImage, egui::TextureHandle)>,
}

impl State {
    pub fn new() -> Self {
        Self {
            rendered_navicust_cache: None,
        }
    }
}

fn navicust_part_colors(color: &rom::NavicustPartColor) -> (image::Rgba<u8>, image::Rgba<u8>) {
    match color {
        rom::NavicustPartColor::Red => (
            image::Rgba([0xde, 0x10, 0x00, 0xff]),
            image::Rgba([0xbd, 0x00, 0x00, 0xff]),
        ),
        rom::NavicustPartColor::Pink => (
            image::Rgba([0xde, 0x8c, 0xc6, 0xff]),
            image::Rgba([0xbd, 0x6b, 0xa5, 0xff]),
        ),
        rom::NavicustPartColor::Yellow => (
            image::Rgba([0xde, 0xde, 0x00, 0xff]),
            image::Rgba([0xbd, 0xbd, 0x00, 0xff]),
        ),
        rom::NavicustPartColor::Green => (
            image::Rgba([0x18, 0xc6, 0x00, 0xff]),
            image::Rgba([0x00, 0xa5, 0x00, 0xff]),
        ),
        rom::NavicustPartColor::Blue => (
            image::Rgba([0x29, 0x84, 0xde, 0xff]),
            image::Rgba([0x08, 0x60, 0xb8, 0xff]),
        ),
        rom::NavicustPartColor::White => (
            image::Rgba([0xde, 0xde, 0xde, 0xff]),
            image::Rgba([0xbd, 0xbd, 0xbd, 0xff]),
        ),
        rom::NavicustPartColor::Orange => (
            image::Rgba([0xde, 0x7b, 0x00, 0xff]),
            image::Rgba([0xbd, 0x5a, 0x00, 0xff]),
        ),
        rom::NavicustPartColor::Purple => (
            image::Rgba([0x94, 0x00, 0xce, 0xff]),
            image::Rgba([0x73, 0x00, 0xad, 0xff]),
        ),
        rom::NavicustPartColor::Gray => (
            image::Rgba([0x84, 0x84, 0x84, 0xff]),
            image::Rgba([0x63, 0x63, 0x63, 0xff]),
        ),
    }
}

fn show_part_name(
    ui: &mut egui::Ui,
    name: egui::RichText,
    description: egui::RichText,
    is_enabled: bool,
    color: &rom::NavicustPartColor,
) {
    egui::Frame::none()
        .inner_margin(egui::style::Margin::symmetric(4.0, 0.0))
        .rounding(egui::Rounding::same(2.0))
        .fill(if is_enabled {
            let (color, _) = navicust_part_colors(color);
            egui::Color32::from_rgb(color.0[0], color.0[1], color.0[2])
        } else {
            egui::Color32::from_rgb(0xbd, 0xbd, 0xbd)
        })
        .show(ui, |ui| {
            ui.label(name.color(egui::Color32::BLACK));
        })
        .response
        .on_hover_text(description);
}

fn ncp_bitmap<'a>(info: &'a Box<dyn rom::NavicustPart + 'a>, compressed: bool, rot: u8) -> rom::NavicustBitmap {
    let mut bitmap = if compressed {
        info.compressed_bitmap()
    } else {
        info.uncompressed_bitmap()
    };

    match rot {
        1 => {
            bitmap = image::imageops::rotate90(&bitmap);
        }
        2 => {
            image::imageops::rotate180_in_place(&mut bitmap);
        }
        3 => {
            bitmap = image::imageops::rotate270(&bitmap);
        }
        _ => {}
    }

    bitmap
}

type ComposedNavicust = image::ImageBuffer<image::LumaA<u8>, Vec<u8>>;

fn compose_navicust<'a>(
    navicust_view: &Box<dyn save::NavicustView<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync + 'a>,
) -> ComposedNavicust {
    let mut composed = image::ImageBuffer::new(navicust_view.width() as u32, navicust_view.height() as u32);
    for i in 0..navicust_view.count() {
        let ncp = if let Some(ncp) = navicust_view.navicust_part(i) {
            ncp
        } else {
            continue;
        };

        let info = if let Some(info) = assets.navicust_part(ncp.id, ncp.variant) {
            info
        } else {
            continue;
        };

        let bitmap = ncp_bitmap(&info, ncp.compressed, ncp.rot);
        let width = bitmap.width();
        let height = bitmap.height();

        // Convert bitmap to composable Navicust image (LumaA).
        image::imageops::overlay(
            &mut composed,
            &image::ImageBuffer::from_vec(
                width,
                height,
                bitmap
                    .into_iter()
                    .flat_map(|b| [i as u8, if *b != 0 { 0xff } else { 0 }])
                    .collect::<Vec<u8>>(),
            )
            .unwrap(),
            ncp.col as i64 - (width / 2) as i64,
            ncp.row as i64 - (height / 2) as i64,
        );
    }
    composed
}

fn render_navicust<'a>(
    composed: &ComposedNavicust,
    navicust_view: &Box<dyn save::NavicustView<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync + 'a>,
) -> image::RgbaImage {
    let mut image = image::ImageBuffer::new(composed.width(), composed.height());
    for (i, p) in composed.pixels().enumerate() {
        let x = i % composed.width() as usize;
        let y = i / composed.width() as usize;
        let [l, a] = p.0;

        if a != 0 {
            let ncp_i = l as usize;
            let ncp = if let Some(ncp) = navicust_view.navicust_part(ncp_i) {
                ncp
            } else {
                continue;
            };

            let info = if let Some(info) = assets.navicust_part(ncp.id, ncp.variant) {
                info
            } else {
                continue;
            };

            let color = if let Some(color) = info.color() {
                color
            } else {
                continue;
            };

            image.put_pixel(x as u32, y as u32, navicust_part_colors(&color).0);
        }
    }
    image
}

pub fn show<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    navicust_view: &Box<dyn save::NavicustView<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    state: &mut State,
) {
    const NCP_CHIP_WIDTH: f32 = 150.0;

    let items = (0..navicust_view.count())
        .flat_map(|i| {
            navicust_view.navicust_part(i).and_then(|ncp| {
                assets
                    .navicust_part(ncp.id, ncp.variant)
                    .and_then(|info| info.color().map(|color| (info, color)))
            })
        })
        .collect::<Vec<_>>();

    ui.horizontal(|ui| {
        if ui
            .button(format!(
                "📋 {}",
                i18n::LOCALES.lookup(lang, "copy-to-clipboard").unwrap(),
            ))
            .clicked()
        {
            let mut buf = vec![];
            if let Some(style) = navicust_view.style() {
                buf.push(
                    assets
                        .style(style)
                        .map(|style| style.name())
                        .unwrap_or_else(|| "".to_string())
                        .to_owned(),
                );
            }
            buf.extend(
                itertools::Itertools::zip_longest(
                    items
                        .iter()
                        .filter(|(info, _)| info.is_solid())
                        .map(|(info, _)| info.name()),
                    items
                        .iter()
                        .filter(|(info, _)| !info.is_solid())
                        .map(|(info, _)| info.name()),
                )
                .map(|v| match v {
                    itertools::EitherOrBoth::Both(l, r) => format!("{}\t{}", l, r),
                    itertools::EitherOrBoth::Left(l) => format!("{}\t", l),
                    itertools::EitherOrBoth::Right(r) => format!("\t{}", r),
                }),
            );
            let _ = clipboard.set_text(buf.join("\n"));
        }

        if ui
            .button(format!(
                "📋 {}",
                i18n::LOCALES.lookup(lang, "copy-navicust-image-to-clipboard").unwrap(),
            ))
            .clicked()
        {
            (|| {
                let image = if let Some((image, _)) = state.rendered_navicust_cache.as_ref() {
                    image
                } else {
                    return;
                };

                let _ = clipboard.set_image(arboard::ImageData {
                    width: image.width() as usize,
                    height: image.height() as usize,
                    bytes: std::borrow::Cow::Borrowed(&image),
                });
            })()
        }
    });

    if let Some(style) = navicust_view.style() {
        ui.label(
            assets
                .style(style)
                .map(|style| style.name())
                .unwrap_or_else(|| "".to_string()),
        );
    }

    ui.horizontal(|ui| {
        if !state.rendered_navicust_cache.is_some() {
            let composed = compose_navicust(navicust_view, assets);
            let image = render_navicust(&composed, navicust_view, assets);
            let texture = ui.ctx().load_texture(
                "navicust",
                egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], &image),
                egui::TextureFilter::Nearest,
            );
            state.rendered_navicust_cache = Some((image, texture));
        }

        if let Some((_, texture_handle)) = state.rendered_navicust_cache.as_ref() {
            ui.image(texture_handle.id(), egui::Vec2::new(70.0, 70.0));
        }

        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            ui.set_width(NCP_CHIP_WIDTH);
            for (info, color) in items.iter().filter(|(info, _)| info.is_solid()) {
                show_part_name(
                    ui,
                    egui::RichText::new(&info.name()).family(font_families.for_language(game_lang)),
                    egui::RichText::new(&info.description()).family(font_families.for_language(game_lang)),
                    true,
                    color,
                );
            }
        });
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            ui.set_width(NCP_CHIP_WIDTH);
            for (info, color) in items.iter().filter(|(info, _)| !info.is_solid()) {
                show_part_name(
                    ui,
                    egui::RichText::new(&info.name()).family(font_families.for_language(game_lang)),
                    egui::RichText::new(&info.description()).family(font_families.for_language(game_lang)),
                    true,
                    color,
                );
            }
        });
    });
}
