use fluent_templates::Loader;
use itertools::Itertools;

use crate::{config, fonts, gui, i18n};

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
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(4, 0))
        .corner_radius(egui::CornerRadius::same(2))
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

fn render_navicust(
    ctx: &egui::Context,
    materialized: &tango_dataview::navicust::MaterializedNavicust,
    navicust_layout: &tango_dataview::rom::NavicustLayout,
    navicust_view: &(dyn tango_dataview::save::NavicustView),
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
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
            let name = info.name().unwrap_or_else(|| "???".to_string());

            let pixels_per_point = ctx.pixels_per_point();

            ctx.fonts(|fonts| {
                let font_size = color_bar.height() as f32 * 2.0 / 3.0 / pixels_per_point;
                let font_id = egui::FontId::new(font_size, egui::FontFamily::Proportional);

                let galley = fonts.layout_no_wrap(name, font_id, egui::Color32::WHITE);

                let atlas = fonts.texture_atlas();
                let atlas = atlas.lock();
                let atlas_image = atlas.image();

                for row in &galley.rows {
                    for glyph in &row.glyphs {
                        // grab the glyph image
                        let [x0, y0] = glyph.uv_rect.min;
                        let [x1, y1] = glyph.uv_rect.max;
                        let w = x1 - x0;
                        let h = y1 - y0;

                        let atlas_region = atlas_image.region([x0 as _, y0 as _], [w as _, h as _]);
                        let coverage = atlas_region.srgba_pixels(None);

                        let g = image::RgbaImage::from_vec(
                            w as _,
                            h as _,
                            coverage.flat_map(|c| [c.r(), c.g(), c.b(), c.a()]).collect(),
                        )
                        .unwrap();

                        // place the glyph
                        let pos = (glyph.pos + glyph.uv_rect.offset) * pixels_per_point;
                        image::imageops::overlay(&mut color_bar, &g, pos.x as _, pos.y as _);
                    }
                }
            });
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

fn gather_ncp_colors(
    navicust_view: &(dyn tango_dataview::save::NavicustView),
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
) -> Vec<tango_dataview::rom::NavicustPartColor> {
    (0..navicust_view.count())
        .flat_map(|i| {
            let ncp = if let Some(ncp) = navicust_view.navicust_part(i) {
                ncp
            } else {
                return vec![];
            };

            let info = if let Some(info) = assets.navicust_part(ncp.id) {
                info
            } else {
                return vec![];
            };

            let color = if let Some(color) = info.color() {
                color
            } else {
                return vec![];
            };

            vec![color]
        })
        .unique()
        .collect::<Vec<_>>()
}

fn render_navicust_color_bar3(extra_color: Option<tango_dataview::rom::NavicustPartColor>) -> image::RgbaImage {
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

    let stroke = tiny_skia::Stroke {
        line_cap: tiny_skia::LineCap::Square,
        width: BORDER_WIDTH,
        ..Default::default()
    };

    let path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, TILE_WIDTH, SQUARE_SIZE / 2.0).unwrap());
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

fn render_navicust_color_bar456(
    navicust_view: &(dyn tango_dataview::save::NavicustView),
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
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

    let stroke = tiny_skia::Stroke {
        line_cap: tiny_skia::LineCap::Square,
        width: BORDER_WIDTH,
        ..Default::default()
    };

    let outline_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, TILE_WIDTH, SQUARE_SIZE / 2.0).unwrap());
        pb.finish().unwrap()
    };

    let tile_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(
            tiny_skia::Rect::from_xywh(
                BORDER_WIDTH / 2.0,
                BORDER_WIDTH / 2.0,
                TILE_WIDTH - BORDER_WIDTH,
                SQUARE_SIZE / 2.0 - BORDER_WIDTH,
            )
            .unwrap(),
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

fn render_navicust_body(
    materialized: &tango_dataview::navicust::MaterializedNavicust,
    navicust_layout: &tango_dataview::rom::NavicustLayout,
    navicust_view: &(dyn tango_dataview::save::NavicustView),
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
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

    let stroke = tiny_skia::Stroke {
        width: BORDER_WIDTH,
        line_cap: tiny_skia::LineCap::Square,
        ..Default::default()
    };

    let square_path = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, SQUARE_SIZE, SQUARE_SIZE).unwrap());
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
            #[allow(clippy::nonminimal_bool)]
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
        let x = i % width;
        let y = i / width;
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

        let info = if let Some(info) = assets.navicust_part(ncp.id) {
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
        let Some(ncp_i) = *ncp_i else {
            continue;
        };

        let x = i % width;
        let y = i / width;

        let transform = root_transform.pre_translate(x as f32 * SQUARE_SIZE, y as f32 * SQUARE_SIZE);

        for neighbor in neighbors.iter() {
            let x = x as isize + neighbor.offset[0];
            let y = y as isize + neighbor.offset[1];

            let mut should_stroke = x < 0 || x >= width as isize || y < 0 || y >= height as isize;
            if !should_stroke
                && materialized[[y as usize, x as usize]]
                    .map(|v| v != ncp_i)
                    .unwrap_or(true)
            {
                should_stroke = true;
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
            pb.push_rect(
                tiny_skia::Rect::from_xywh(-BORDER_WIDTH / 2.0, 1.0 * SQUARE_SIZE - BORDER_WIDTH / 2.0, w, h).unwrap(),
            );

            // Right
            pb.push_rect(
                tiny_skia::Rect::from_xywh(
                    (width - 1) as f32 * SQUARE_SIZE - BORDER_WIDTH / 2.0,
                    1.0 * SQUARE_SIZE - BORDER_WIDTH / 2.0,
                    w,
                    h,
                )
                .unwrap(),
            );

            // Top
            pb.push_rect(
                tiny_skia::Rect::from_xywh(1.0 * SQUARE_SIZE - BORDER_WIDTH / 2.0, -BORDER_WIDTH / 2.0, h, w).unwrap(),
            );

            // Bottom
            pb.push_rect(
                tiny_skia::Rect::from_xywh(
                    1.0 * SQUARE_SIZE - BORDER_WIDTH / 2.0,
                    (height - 1) as f32 * SQUARE_SIZE - BORDER_WIDTH / 2.0,
                    h,
                    w,
                )
                .unwrap(),
            );

            pb.finish().unwrap()
        };

        let mut oob_paint = tiny_skia::Paint::default();
        oob_paint.set_color_rgba8(0x00, 0x00, 0x00, 0x80);

        pixmap.fill_path(&path, &oob_paint, tiny_skia::FillRule::Winding, root_transform, None);
    }

    image::ImageBuffer::from_raw(pixmap.width(), pixmap.height(), pixmap.take()).unwrap()
}

pub fn show(
    ui: &mut egui::Ui,
    config: &config::Config,
    shared_root_state: &mut gui::SharedRootState,
    game_lang: &unic_langid::LanguageIdentifier,
    navicust_view: &dyn tango_dataview::save::NavicustView,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    state: &mut State,
    prefer_vertical: bool,
) {
    let lang = &config.language;
    let clipboard = &mut shared_root_state.clipboard;
    let font_families = &shared_root_state.font_families;

    let Some(navicust_layout) = assets.navicust_layout() else {
        return;
    };

    let items = (0..navicust_view.count())
        .flat_map(|i| {
            navicust_view.navicust_part(i).and_then(|ncp| {
                assets
                    .navicust_part(ncp.id)
                    .and_then(|info| info.color().map(|color| (info, color)))
            })
        })
        .collect::<Vec<_>>();

    ui.horizontal(|ui| {
        let as_text_text = i18n::LOCALES.lookup(lang, "copy-to-clipboard.as-text").unwrap();
        let as_image_text = i18n::LOCALES.lookup(lang, "copy-to-clipboard.as-image").unwrap();

        let navi_cust_grid_args = [(
            "name",
            i18n::LOCALES.lookup(lang, "save-tab-navi-cust-grid").unwrap().into(),
        )]
        .into();

        let grid_as_image_text = i18n::LOCALES
            .lookup_with_args(lang, "copy-to-clipboard.named-as-image", &navi_cust_grid_args)
            .unwrap();

        if ui.button(as_text_text).clicked() {
            ui.close_menu();

            let mut buf = vec![];
            if let Some(style) = navicust_view.style() {
                buf.push(
                    assets
                        .style(style)
                        .and_then(|style| style.name())
                        .unwrap_or_else(|| "".to_string())
                        .to_owned(),
                );
            }
            buf.extend(
                itertools::Itertools::zip_longest(
                    items
                        .iter()
                        .filter(|(info, _)| info.is_solid())
                        .map(|(info, _)| info.name().unwrap_or_else(|| "???".to_string())),
                    items
                        .iter()
                        .filter(|(info, _)| !info.is_solid())
                        .map(|(info, _)| info.name().unwrap_or_else(|| "???".to_string())),
                )
                .map(|v| match v {
                    itertools::EitherOrBoth::Both(l, r) => format!("{}\t{}", l, r),
                    itertools::EitherOrBoth::Left(l) => format!("{}\t", l),
                    itertools::EitherOrBoth::Right(r) => format!("\t{}", r),
                }),
            );
            let _ = clipboard.set_text(buf.join("\n"));
        }

        if ui.button(as_image_text).clicked() {
            ui.close_menu();

            shared_root_state.offscreen_ui.resize(0, 0);
            shared_root_state.offscreen_ui.run(|ui| {
                egui::Frame::new()
                    .fill(ui.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(8, 8))
                    .show(ui, |ui| {
                        show_navicust_view(
                            ui,
                            font_families,
                            game_lang,
                            navicust_view,
                            &navicust_layout,
                            assets,
                            &items,
                            &mut State::new(),
                            prefer_vertical,
                        );
                    });
            });
            shared_root_state.offscreen_ui.copy_to_clipboard();
            shared_root_state.offscreen_ui.sweep();
        }

        if ui.button(grid_as_image_text).clicked() {
            ui.close_menu();

            if let Some((image, _, _)) = state.rendered_navicust_cache.as_ref() {
                let _ = clipboard.set_image(arboard::ImageData {
                    width: image.width() as usize,
                    height: image.height() as usize,
                    bytes: std::borrow::Cow::Borrowed(image),
                });
            };
        }
    });

    egui::ScrollArea::vertical()
        .id_salt("navicust-view")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            show_navicust_view(
                ui,
                font_families,
                game_lang,
                navicust_view,
                &navicust_layout,
                assets,
                &items,
                state,
                prefer_vertical,
            );
        });
}

fn show_navicust_view(
    ui: &mut egui::Ui,
    font_families: &fonts::FontFamilies,
    game_lang: &unic_langid::LanguageIdentifier,
    navicust_view: &dyn tango_dataview::save::NavicustView,
    navicust_layout: &tango_dataview::rom::NavicustLayout,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    items: &[(
        Box<dyn tango_dataview::rom::NavicustPart + '_>,
        tango_dataview::rom::NavicustPartColor,
    )],
    state: &mut State,
    prefer_vertical: bool,
) {
    ui.with_layout(
        if prefer_vertical {
            egui::Layout::top_down(egui::Align::Min)
        } else {
            egui::Layout::left_to_right(egui::Align::Min)
        },
        |ui| {
            if state.rendered_navicust_cache.is_none() {
                let materialized = navicust_view.materialized();
                let image = render_navicust(ui.ctx(), &materialized, navicust_layout, navicust_view, assets);
                let texture = ui.ctx().load_texture(
                    "navicust",
                    egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], &image),
                    egui::TextureOptions::NEAREST,
                );
                state.rendered_navicust_cache = Some((image, materialized, texture));
            }

            if let Some((image, materialized, texture_handle)) = state.rendered_navicust_cache.as_ref() {
                let resp = ui.image((
                    texture_handle.id(),
                    egui::Vec2::new((image.width() / 2) as f32, (image.height() / 2) as f32),
                ));
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
                                .and_then(|ncp| assets.navicust_part(ncp.id))
                            {
                                resp.on_hover_text_at_pointer(
                                    egui::RichText::new(info.name().unwrap_or_else(|| "???".to_string()))
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
                            egui::RichText::new(info.name().unwrap_or_else(|| "???".to_string()))
                                .family(font_families.for_language(game_lang)),
                            egui::RichText::new(info.description().unwrap_or_else(|| "???".to_string()))
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
                            egui::RichText::new(info.name().unwrap_or_else(|| "???".to_string()))
                                .family(font_families.for_language(game_lang)),
                            egui::RichText::new(info.description().unwrap_or_else(|| "???".to_string()))
                                .family(font_families.for_language(game_lang)),
                            true,
                            color,
                        );
                    }
                });
            });
        },
    );
}
