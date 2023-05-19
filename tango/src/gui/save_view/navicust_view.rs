use fluent_templates::Loader;
use itertools::Itertools;

use crate::{gui, i18n};

pub struct State {
    rendered_navicust_cache: Option<(
        image::RgbaImage,
        tango_dataview::navicust::MaterializedNavicust,
        egui::TextureHandle,
    )>,
}

impl State {
    pub fn new() -> Self {
        Self {
            rendered_navicust_cache: None,
        }
    }
}

fn navicust_part_colors(color: &tango_dataview::rom::NavicustPartColor) -> (image::Rgba<u8>, image::Rgba<u8>) {
    match color {
        tango_dataview::rom::NavicustPartColor::Red => (
            image::Rgba([0xde, 0x10, 0x00, 0xff]),
            image::Rgba([0xbd, 0x00, 0x00, 0xff]),
        ),
        tango_dataview::rom::NavicustPartColor::Pink => (
            image::Rgba([0xde, 0x8c, 0xc6, 0xff]),
            image::Rgba([0xbd, 0x6b, 0xa5, 0xff]),
        ),
        tango_dataview::rom::NavicustPartColor::Yellow => (
            image::Rgba([0xde, 0xde, 0x00, 0xff]),
            image::Rgba([0xbd, 0xbd, 0x00, 0xff]),
        ),
        tango_dataview::rom::NavicustPartColor::Green => (
            image::Rgba([0x18, 0xc6, 0x00, 0xff]),
            image::Rgba([0x00, 0xa5, 0x00, 0xff]),
        ),
        tango_dataview::rom::NavicustPartColor::Blue => (
            image::Rgba([0x29, 0x84, 0xde, 0xff]),
            image::Rgba([0x08, 0x60, 0xb8, 0xff]),
        ),
        tango_dataview::rom::NavicustPartColor::White => (
            image::Rgba([0xde, 0xde, 0xde, 0xff]),
            image::Rgba([0xbd, 0xbd, 0xbd, 0xff]),
        ),
        tango_dataview::rom::NavicustPartColor::Orange => (
            image::Rgba([0xde, 0x7b, 0x00, 0xff]),
            image::Rgba([0xbd, 0x5a, 0x00, 0xff]),
        ),
        tango_dataview::rom::NavicustPartColor::Purple => (
            image::Rgba([0x94, 0x00, 0xce, 0xff]),
            image::Rgba([0x73, 0x00, 0xad, 0xff]),
        ),
        tango_dataview::rom::NavicustPartColor::Gray => (
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
    color: &tango_dataview::rom::NavicustPartColor,
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

const PADDING_H: u32 = 20;
const PADDING_V: u32 = 20;

const BORDER_WIDTH: f32 = 6.0;
const SQUARE_SIZE: f32 = 60.0;

const BG_FILL_COLOR: image::Rgba<u8> = image::Rgba([0x20, 0x20, 0x20, 0xff]);
const BORDER_STROKE_COLOR: image::Rgba<u8> = image::Rgba([0x00, 0x00, 0x00, 0xff]);

fn render_navicust<'a>(
    materialized: &tango_dataview::navicust::MaterializedNavicust,
    navicust_layout: &tango_dataview::rom::NavicustLayout,
    navicust_view: &Box<dyn tango_dataview::save::NavicustView<'a> + 'a>,
    assets: &Box<dyn tango_dataview::rom::Assets + Send + Sync + 'a>,
    raw_font: &[u8],
) -> image::RgbaImage {
    let body = render_navicust_body(materialized, navicust_layout, navicust_view, assets);

    let color_bar = if let Some(style) = navicust_view.style() {
        let color_bar_right = render_navicust_color_bar3(assets.style(style).and_then(|style| style.extra_ncp_color()));
        let mut color_bar = image::RgbaImage::new(body.width(), color_bar_right.height());
        let width = color_bar.width();
        image::imageops::overlay(
            &mut color_bar,
            &color_bar_right,
            (width - color_bar_right.width()) as i64,
            0,
        );

        if let Some(info) = assets.style(style) {
            let font = fontdue::Font::from_bytes(raw_font, fontdue::FontSettings::default()).unwrap();
            let px = color_bar.height() as f32 * 2.0 / 3.0;
            let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
            layout.append(&[&font], &fontdue::layout::TextStyle::new(&info.name(), px, 0));

            for glyph in layout.glyphs() {
                let (metrics, coverage) = font.rasterize(glyph.parent, px);
                let g = image::RgbaImage::from_vec(
                    metrics.width as u32,
                    metrics.height as u32,
                    coverage.into_iter().flat_map(|a| [0xff, 0xff, 0xff, a]).collect(),
                )
                .unwrap();
                image::imageops::overlay(&mut color_bar, &g, glyph.x as i64, glyph.y as i64);
            }
        }

        color_bar
    } else {
        render_navicust_color_bar456(navicust_view, assets)
    };

    let mut image = image::RgbaImage::new(
        body.width() + PADDING_H * 2,
        body.height() + PADDING_V * 2 + color_bar.height() + PADDING_V,
    );

    for pixel in image.pixels_mut() {
        *pixel = navicust_layout.background;
    }

    image::imageops::overlay(&mut image, &color_bar, PADDING_H as i64, PADDING_V as i64);
    image::imageops::overlay(
        &mut image,
        &body,
        PADDING_H as i64,
        (PADDING_V + color_bar.height() + PADDING_V) as i64,
    );

    image
}

fn gather_ncp_colors<'a>(
    navicust_view: &Box<dyn tango_dataview::save::NavicustView<'a> + 'a>,
    assets: &Box<dyn tango_dataview::rom::Assets + Send + Sync + 'a>,
) -> Vec<tango_dataview::rom::NavicustPartColor> {
    (0..navicust_view.count())
        .flat_map(|i| {
            let ncp = if let Some(ncp) = navicust_view.navicust_part(i) {
                ncp
            } else {
                return vec![];
            };

            let info = if let Some(info) = assets.navicust_part(ncp.id, ncp.variant) {
                info
            } else {
                return vec![];
            };

            let color = if let Some(color) = info.color() {
                color
            } else {
                return vec![];
            };

            return vec![color];
        })
        .unique()
        .collect::<Vec<_>>()
}

fn render_navicust_color_bar3<'a>(extra_color: Option<tango_dataview::rom::NavicustPartColor>) -> image::RgbaImage {
    const TILE_WIDTH: f32 = SQUARE_SIZE / 4.0;

    let mut pixmap = tiny_skia::Pixmap::new(
        (TILE_WIDTH * 4.0 + BORDER_WIDTH) as u32,
        (SQUARE_SIZE / 2.0 + BORDER_WIDTH) as u32,
    )
    .unwrap();

    let mut bg_fill_paint = tiny_skia::Paint::default();
    bg_fill_paint.set_color_rgba8(
        BG_FILL_COLOR.0[0],
        BG_FILL_COLOR.0[1],
        BG_FILL_COLOR.0[2],
        BG_FILL_COLOR.0[3],
    );

    let mut border_stroke_paint = tiny_skia::Paint::default();
    border_stroke_paint.set_color_rgba8(
        BORDER_STROKE_COLOR.0[0],
        BORDER_STROKE_COLOR.0[1],
        BORDER_STROKE_COLOR.0[2],
        BORDER_STROKE_COLOR.0[3],
    );

    let mut stroke = tiny_skia::Stroke::default();
    stroke.width = BORDER_WIDTH as f32;
    stroke.line_cap = tiny_skia::LineCap::Square;

    let path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(0.0, 0.0, TILE_WIDTH, SQUARE_SIZE / 2.0);
        pb.finish().unwrap()
    };

    let root_transform = tiny_skia::Transform::from_translate(BORDER_WIDTH / 2.0, BORDER_WIDTH / 2.0);

    for (i, color) in [
        Some(tango_dataview::rom::NavicustPartColor::White),
        Some(tango_dataview::rom::NavicustPartColor::Pink),
        Some(tango_dataview::rom::NavicustPartColor::Yellow),
        extra_color,
    ]
    .into_iter()
    .enumerate()
    {
        let transform = root_transform.pre_translate(i as f32 * TILE_WIDTH, 0.0);
        pixmap.fill_path(
            &path,
            &if let Some(color) = color {
                let (_, plus_color) = navicust_part_colors(&color);
                let mut fill_paint = tiny_skia::Paint::default();
                fill_paint.set_color_rgba8(plus_color.0[0], plus_color.0[1], plus_color.0[2], plus_color.0[3]);
                fill_paint
            } else {
                bg_fill_paint.clone()
            },
            tiny_skia::FillRule::Winding,
            transform,
            None,
        );
        pixmap.stroke_path(&path, &border_stroke_paint, &stroke, transform, None);
    }

    image::ImageBuffer::from_raw(pixmap.width(), pixmap.height(), pixmap.take()).unwrap()
}

fn render_navicust_color_bar456<'a>(
    navicust_view: &Box<dyn tango_dataview::save::NavicustView<'a> + 'a>,
    assets: &Box<dyn tango_dataview::rom::Assets + Send + Sync + 'a>,
) -> image::RgbaImage {
    const TILE_WIDTH: f32 = SQUARE_SIZE * 3.0 / 4.0;

    let colors = gather_ncp_colors(navicust_view, assets);
    let mut pixmap = tiny_skia::Pixmap::new(
        TILE_WIDTH as u32 * std::cmp::max(4, colors.len()) as u32 + BORDER_WIDTH as u32 + BORDER_WIDTH as u32,
        (SQUARE_SIZE / 2.0 + BORDER_WIDTH) as u32,
    )
    .unwrap();

    let nonbug_colors = &colors[..std::cmp::min(colors.len(), 4)];
    let bug_colors = colors.get(4..).unwrap_or(&[]);

    let root_transform = tiny_skia::Transform::from_translate(BORDER_WIDTH / 2.0, BORDER_WIDTH / 2.0);

    let mut bg_fill_paint = tiny_skia::Paint::default();
    bg_fill_paint.set_color_rgba8(
        BG_FILL_COLOR.0[0],
        BG_FILL_COLOR.0[1],
        BG_FILL_COLOR.0[2],
        BG_FILL_COLOR.0[3],
    );

    let mut border_stroke_paint = tiny_skia::Paint::default();
    border_stroke_paint.set_color_rgba8(
        BORDER_STROKE_COLOR.0[0],
        BORDER_STROKE_COLOR.0[1],
        BORDER_STROKE_COLOR.0[2],
        BORDER_STROKE_COLOR.0[3],
    );

    let mut stroke = tiny_skia::Stroke::default();
    stroke.width = BORDER_WIDTH as f32;
    stroke.line_cap = tiny_skia::LineCap::Square;

    let outline_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(0.0, 0.0, TILE_WIDTH, SQUARE_SIZE / 2.0);
        pb.finish().unwrap()
    };

    let tile_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(
            BORDER_WIDTH / 2.0,
            BORDER_WIDTH / 2.0,
            TILE_WIDTH - BORDER_WIDTH,
            SQUARE_SIZE / 2.0 - BORDER_WIDTH,
        );
        pb.finish().unwrap()
    };

    for i in 0..4 {
        let transform = root_transform.pre_translate(i as f32 * TILE_WIDTH, 0.0);
        pixmap.fill_path(
            &tile_path,
            &if let Some(color) = nonbug_colors.get(i) {
                let (_, plus_color) = navicust_part_colors(color);
                let mut fill_paint = tiny_skia::Paint::default();
                fill_paint.set_color_rgba8(plus_color.0[0], plus_color.0[1], plus_color.0[2], plus_color.0[3]);
                fill_paint
            } else {
                bg_fill_paint.clone()
            },
            tiny_skia::FillRule::Winding,
            transform,
            None,
        );
        pixmap.stroke_path(&outline_path, &border_stroke_paint, &stroke, transform, None);
    }

    for (i, bug_color) in bug_colors.iter().enumerate() {
        let transform = root_transform.pre_translate((i + 4) as f32 * TILE_WIDTH + BORDER_WIDTH, 0.0);
        pixmap.fill_path(
            &tile_path,
            &{
                let (_, plus_color) = navicust_part_colors(bug_color);
                let mut fill_paint = tiny_skia::Paint::default();
                fill_paint.set_color_rgba8(plus_color.0[0], plus_color.0[1], plus_color.0[2], plus_color.0[3]);
                fill_paint
            },
            tiny_skia::FillRule::Winding,
            transform,
            None,
        );
    }

    image::ImageBuffer::from_raw(pixmap.width(), pixmap.height(), pixmap.take()).unwrap()
}

fn render_navicust_body<'a>(
    materialized: &tango_dataview::navicust::MaterializedNavicust,
    navicust_layout: &tango_dataview::rom::NavicustLayout,
    navicust_view: &Box<dyn tango_dataview::save::NavicustView<'a> + 'a>,
    assets: &Box<dyn tango_dataview::rom::Assets + Send + Sync + 'a>,
) -> image::RgbaImage {
    let (height, width) = materialized.dim();

    let mut pixmap = tiny_skia::Pixmap::new(
        (width as f32 * SQUARE_SIZE + BORDER_WIDTH) as u32,
        (height as f32 * SQUARE_SIZE + BORDER_WIDTH) as u32,
    )
    .unwrap();

    let root_transform = tiny_skia::Transform::from_translate(BORDER_WIDTH / 2.0, BORDER_WIDTH / 2.0);

    let mut bg_fill_paint = tiny_skia::Paint::default();
    bg_fill_paint.set_color_rgba8(
        BG_FILL_COLOR.0[0],
        BG_FILL_COLOR.0[1],
        BG_FILL_COLOR.0[2],
        BG_FILL_COLOR.0[3],
    );

    let mut border_stroke_paint = tiny_skia::Paint::default();
    border_stroke_paint.set_color_rgba8(
        BORDER_STROKE_COLOR.0[0],
        BORDER_STROKE_COLOR.0[1],
        BORDER_STROKE_COLOR.0[2],
        BORDER_STROKE_COLOR.0[3],
    );

    let mut stroke = tiny_skia::Stroke::default();
    stroke.width = BORDER_WIDTH as f32;
    stroke.line_cap = tiny_skia::LineCap::Square;

    let square_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(0.0, 0.0, SQUARE_SIZE, SQUARE_SIZE);
        pb.finish().unwrap()
    };

    let plus_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(SQUARE_SIZE / 2.0, 0.0);
        pb.line_to(SQUARE_SIZE / 2.0, SQUARE_SIZE);
        pb.move_to(0.0, SQUARE_SIZE / 2.0);
        pb.line_to(SQUARE_SIZE, SQUARE_SIZE / 2.0);
        pb.finish().unwrap()
    };

    let command_line_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.move_to(0.0, 0.0);
        pb.line_to(SQUARE_SIZE * width as f32, 0.0);
        pb.finish().unwrap()
    };

    struct Neighbor {
        offset: [isize; 2],
        border_path: tiny_skia::Path,
    }

    let neighbors = [
        Neighbor {
            offset: [0, -1],
            border_path: {
                let mut pb = tiny_skia::PathBuilder::new();
                pb.move_to(0.0, 0.0);
                pb.line_to(SQUARE_SIZE, 0.0);
                pb.finish().unwrap()
            },
        },
        Neighbor {
            offset: [-1, 0],
            border_path: {
                let mut pb = tiny_skia::PathBuilder::new();
                pb.move_to(0.0, 0.0);
                pb.line_to(0.0, SQUARE_SIZE);
                pb.finish().unwrap()
            },
        },
        Neighbor {
            offset: [0, 1],
            border_path: {
                let mut pb = tiny_skia::PathBuilder::new();
                pb.move_to(0.0, SQUARE_SIZE);
                pb.line_to(SQUARE_SIZE, SQUARE_SIZE);
                pb.finish().unwrap()
            },
        },
        Neighbor {
            offset: [1, 0],
            border_path: {
                let mut pb = tiny_skia::PathBuilder::new();
                pb.move_to(SQUARE_SIZE, 0.0);
                pb.line_to(SQUARE_SIZE, SQUARE_SIZE);
                pb.finish().unwrap()
            },
        },
    ];

    // First pass: draw background.
    for y in 0..width {
        for x in 0..height {
            if navicust_layout.has_out_of_bounds
                && ((x == 0 && y == 0)
                    || (x == 0 && y == height - 1)
                    || (x == width - 1 && y == 0)
                    || (x == width - 1 && y == height - 1))
            {
                continue;
            }

            let transform = root_transform.pre_translate(x as f32 * SQUARE_SIZE, y as f32 * SQUARE_SIZE);

            pixmap.fill_path(
                &square_path,
                &bg_fill_paint,
                tiny_skia::FillRule::Winding,
                transform,
                None,
            );
            pixmap.stroke_path(&square_path, &border_stroke_paint, &stroke, transform, None);
        }
    }

    // Second pass: draw squares.
    for (i, ncp_i) in materialized.iter().enumerate() {
        let x = i % width as usize;
        let y = i / width as usize;
        let ncp_i = if let Some(ncp_i) = ncp_i {
            *ncp_i
        } else {
            continue;
        };

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

        let transform = root_transform.pre_translate(x as f32 * SQUARE_SIZE, y as f32 * SQUARE_SIZE);

        let (solid_color, plus_color) = navicust_part_colors(&color);
        let mut fill_paint = tiny_skia::Paint::default();
        fill_paint.set_color_rgba8(solid_color.0[0], solid_color.0[1], solid_color.0[2], solid_color.0[3]);

        let mut stroke_paint = tiny_skia::Paint::default();
        stroke_paint.set_color_rgba8(plus_color.0[0], plus_color.0[1], plus_color.0[2], plus_color.0[3]);

        pixmap.fill_path(&square_path, &fill_paint, tiny_skia::FillRule::Winding, transform, None);
        pixmap.stroke_path(&square_path, &stroke_paint, &stroke, transform, None);
        if !info.is_solid() {
            pixmap.stroke_path(&plus_path, &stroke_paint, &stroke, transform, None);
        }
    }

    // Third pass: draw borders.
    for (i, ncp_i) in materialized.iter().enumerate() {
        let ncp_i = if let Some(ncp_i) = ncp_i {
            *ncp_i
        } else {
            continue;
        };

        let x = i % width as usize;
        let y = i / width as usize;

        let transform = root_transform.pre_translate(x as f32 * SQUARE_SIZE, y as f32 * SQUARE_SIZE);

        for neighbor in neighbors.iter() {
            let x = x as isize + neighbor.offset[0];
            let y = y as isize + neighbor.offset[1];

            let mut should_stroke = x < 0 || x >= width as isize || y < 0 || y >= height as isize;
            if !should_stroke {
                if materialized[[y as usize, x as usize]]
                    .map(|v| v != ncp_i)
                    .unwrap_or(true)
                {
                    should_stroke = true;
                }
            }

            if should_stroke {
                pixmap.stroke_path(&neighbor.border_path, &border_stroke_paint, &stroke, transform, None);
            }
        }
    }

    // Fourth pass: draw command line.
    let command_line_top = navicust_layout.command_line as f32 * SQUARE_SIZE;
    pixmap.stroke_path(
        &command_line_path,
        &border_stroke_paint,
        &stroke,
        root_transform.pre_translate(0.0, command_line_top + SQUARE_SIZE * 1.0 / 4.0),
        None,
    );
    pixmap.stroke_path(
        &command_line_path,
        &border_stroke_paint,
        &stroke,
        root_transform.pre_translate(0.0, command_line_top + SQUARE_SIZE * 3.0 / 4.0),
        None,
    );

    // Fifth pass: draw out of bounds overlay.
    if navicust_layout.has_out_of_bounds {
        let path = {
            let mut pb = tiny_skia::PathBuilder::new();

            let w = SQUARE_SIZE + BORDER_WIDTH;
            let h = (height - 2) as f32 * SQUARE_SIZE + BORDER_WIDTH;

            // Left
            pb.push_rect(-BORDER_WIDTH / 2.0, 1.0 * SQUARE_SIZE - BORDER_WIDTH / 2.0, w, h);

            // Right
            pb.push_rect(
                (width - 1) as f32 * SQUARE_SIZE - BORDER_WIDTH / 2.0,
                1.0 * SQUARE_SIZE - BORDER_WIDTH / 2.0,
                w,
                h,
            );

            // Top
            pb.push_rect(1.0 * SQUARE_SIZE - BORDER_WIDTH / 2.0, -BORDER_WIDTH / 2.0, h, w);

            // Bottom
            pb.push_rect(
                1.0 * SQUARE_SIZE - BORDER_WIDTH / 2.0,
                (height - 1) as f32 * SQUARE_SIZE - BORDER_WIDTH / 2.0,
                h,
                w,
            );

            pb.finish().unwrap()
        };

        let mut oob_paint = tiny_skia::Paint::default();
        oob_paint.set_color_rgba8(0x00, 0x00, 0x00, 0x80);

        pixmap.fill_path(&path, &oob_paint, tiny_skia::FillRule::Winding, root_transform, None);
    }

    image::ImageBuffer::from_raw(pixmap.width(), pixmap.height(), pixmap.take()).unwrap()
}

pub fn show<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    navicust_view: &Box<dyn tango_dataview::save::NavicustView<'a> + 'a>,
    assets: &Box<dyn tango_dataview::rom::Assets + Send + Sync>,
    state: &mut State,
    prefer_vertical: bool,
) {
    let navicust_layout = if let Some(navicust_layout) = assets.navicust_layout() {
        navicust_layout
    } else {
        return;
    };

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
                let image = if let Some((image, _, _)) = state.rendered_navicust_cache.as_ref() {
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

    egui::ScrollArea::vertical()
        .id_source("navicust-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.with_layout(
                if prefer_vertical {
                    egui::Layout::top_down(egui::Align::Min)
                } else {
                    egui::Layout::left_to_right(egui::Align::Min)
                },
                |ui| {
                    if !state.rendered_navicust_cache.is_some() {
                        let materialized = navicust_view.materialized().unwrap_or_else(|| {
                            tango_dataview::navicust::materialize(navicust_view.as_ref(), assets.as_ref())
                        });
                        let image = render_navicust(
                            &materialized,
                            &navicust_layout,
                            navicust_view,
                            assets,
                            font_families.raw_for_language(game_lang),
                        );
                        let texture = ui.ctx().load_texture(
                            "navicust",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [image.width() as usize, image.height() as usize],
                                &image,
                            ),
                            egui::TextureOptions::NEAREST,
                        );
                        state.rendered_navicust_cache = Some((image, materialized, texture));
                    }

                    if let Some((image, materialized, texture_handle)) = state.rendered_navicust_cache.as_ref() {
                        let resp = ui.image(
                            texture_handle.id(),
                            egui::Vec2::new((image.width() / 2) as f32, (image.height() / 2) as f32),
                        );
                        if let Some(hover_pos) = resp.hover_pos() {
                            let x = ((hover_pos.x - resp.rect.min.x) * 2.0) as u32;
                            let y = ((hover_pos.y - resp.rect.min.y) * 2.0) as u32;

                            const LEFT: u32 = PADDING_H + (BORDER_WIDTH / 2.0) as u32;
                            const TOP: u32 = PADDING_V
                                + (SQUARE_SIZE / 2.0) as u32
                                + BORDER_WIDTH as u32
                                + PADDING_V
                                + (BORDER_WIDTH / 2.0) as u32;

                            if x >= LEFT
                                && x < image.width() - PADDING_H - (BORDER_WIDTH / 2.0) as u32
                                && y >= TOP
                                && y < image.height() - PADDING_V - (BORDER_WIDTH / 2.0) as u32
                            {
                                let tx = (x - LEFT) / SQUARE_SIZE as u32;
                                let ty = (y - TOP) / SQUARE_SIZE as u32;

                                if let Some(ncp_i) = materialized[[ty as usize, tx as usize]] {
                                    if let Some(info) = navicust_view
                                        .navicust_part(ncp_i)
                                        .and_then(|ncp| assets.navicust_part(ncp.id, ncp.variant))
                                    {
                                        resp.on_hover_text_at_pointer(
                                            egui::RichText::new(&info.name())
                                                .family(font_families.for_language(game_lang)),
                                        );
                                    }
                                }
                            }
                        }
                    }

                    const NCP_CHIP_WIDTH: f32 = 150.0;

                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                            ui.set_width(NCP_CHIP_WIDTH);
                            for (info, color) in items.iter().filter(|(info, _)| info.is_solid()) {
                                show_part_name(
                                    ui,
                                    egui::RichText::new(&info.name()).family(font_families.for_language(game_lang)),
                                    egui::RichText::new(&info.description())
                                        .family(font_families.for_language(game_lang)),
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
                                    egui::RichText::new(&info.description())
                                        .family(font_families.for_language(game_lang)),
                                    true,
                                    color,
                                );
                            }
                        });
                    });
                },
            );
        });
}
