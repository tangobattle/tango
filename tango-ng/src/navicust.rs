//! NaviCust grid rendering. Ported from `tango/src/gui/save_view/navi_view/navicust_view.rs`.
//! For BN3 the color bar carries the style name on its left
//! edge — rasterized via ab_glyph against the bundled NotoSans
//! font so the baked image still reads correctly when copied
//! or exported. Outputs an RGBA image we can hand to iced's
//! image widget.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use std::sync::LazyLock;
use tango_dataview::{
    navicust::MaterializedNavicust,
    rom::{Assets, NavicustLayout, NavicustPartColor},
    save::NavicustView,
};

/// Font used to bake the BN3 style label onto the color bar.
/// Same Noto Sans face the rest of the UI uses, so the on-image
/// label visually matches the iced widgets around it.
static LABEL_FONT: LazyLock<FontRef<'static>> = LazyLock::new(|| {
    FontRef::try_from_slice(include_bytes!("../../tango/fonts/NotoSans-Regular.ttf"))
        .expect("bundled Noto Sans is a valid TTF")
});

const BORDER_WIDTH: f32 = 6.0;
const SQUARE_SIZE: f32 = 60.0;

const BG_FILL_COLOR: [u8; 4] = [0x20, 0x20, 0x20, 0xff];
const BORDER_STROKE_COLOR: [u8; 4] = [0x00, 0x00, 0x00, 0xff];

/// Solid color (filled square) + plus/stroke color, matching the egui app.
fn part_colors(color: NavicustPartColor) -> ([u8; 4], [u8; 4]) {
    match color {
        NavicustPartColor::Red => ([0xde, 0x10, 0x00, 0xff], [0xbd, 0x00, 0x00, 0xff]),
        NavicustPartColor::Pink => ([0xde, 0x8c, 0xc6, 0xff], [0xbd, 0x6b, 0xa5, 0xff]),
        NavicustPartColor::Yellow => ([0xde, 0xde, 0x00, 0xff], [0xbd, 0xbd, 0x00, 0xff]),
        NavicustPartColor::Green => ([0x18, 0xc6, 0x00, 0xff], [0x00, 0xa5, 0x00, 0xff]),
        NavicustPartColor::Blue => ([0x29, 0x84, 0xde, 0xff], [0x08, 0x60, 0xb8, 0xff]),
        NavicustPartColor::White => ([0xde, 0xde, 0xde, 0xff], [0xbd, 0xbd, 0xbd, 0xff]),
        NavicustPartColor::Orange => ([0xde, 0x7b, 0x00, 0xff], [0xbd, 0x5a, 0x00, 0xff]),
        NavicustPartColor::Purple => ([0x94, 0x00, 0xce, 0xff], [0x73, 0x00, 0xad, 0xff]),
        NavicustPartColor::Gray => ([0x84, 0x84, 0x84, 0xff], [0x63, 0x63, 0x63, 0xff]),
    }
}

const PADDING_H: u32 = 20;
const PADDING_V: u32 = 20;

/// Background fill + color bar + body. For BN3 (style is Some)
/// the bar is widened to span the body and the style name is
/// rasterized on the left edge — same layout the legacy egui
/// app produces.
pub fn render(
    materialized: &MaterializedNavicust,
    layout: &NavicustLayout,
    view: &dyn NavicustView,
    assets: &dyn Assets,
) -> image::RgbaImage {
    let body = render_grid(materialized, layout, view, assets);

    let color_bar = if let Some(style_id) = view.style() {
        let extra_color = assets.style(style_id).and_then(|s| s.extra_ncp_color());
        let style_name = assets.style(style_id).and_then(|s| s.name());
        let tiles = render_color_bar_bn3(extra_color);
        // Widen the bar to the body width so the tiles can sit
        // flush against the right edge while the style name fits
        // on the left. Same shape the legacy app produces.
        let mut bar = image::RgbaImage::new(body.width(), tiles.height());
        let bar_w = bar.width();
        let bar_h = bar.height();
        image::imageops::overlay(
            &mut bar,
            &tiles,
            (bar_w - tiles.width()) as i64,
            0,
        );
        if let Some(name) = style_name {
            // Leave a small pad so the glyphs don't kiss the
            // left edge / the tile stripe.
            let label_pad = (BORDER_WIDTH as i64) + 4;
            let max_label_w = (bar_w as i64) - tiles.width() as i64 - label_pad * 2;
            if max_label_w > 0 {
                // Noto Sans line height = ascent − descent ≈ 1.36 ×
                // em, so an em equal to the bar height would crop
                // top + bottom. 0.72 leaves a hair of margin while
                // keeping the label visually present.
                let font_height = (bar_h as f32) * 0.72;
                rasterize_label(&mut bar, &name, label_pad, font_height, max_label_w as u32);
            }
        }
        bar
    } else {
        render_color_bar_bn456(body.width(), view, assets)
    };

    let total_w = body.width() + PADDING_H * 2;
    let total_h = body.height() + PADDING_V * 3 + color_bar.height();
    let mut out = image::RgbaImage::new(total_w, total_h);
    for px in out.pixels_mut() {
        *px = layout.background;
    }
    image::imageops::overlay(&mut out, &color_bar, PADDING_H as i64, PADDING_V as i64);
    image::imageops::overlay(
        &mut out,
        &body,
        PADDING_H as i64,
        (PADDING_V + color_bar.height() + PADDING_V) as i64,
    );
    out
}

/// Blit a left-aligned white label onto `dst` starting at `x0`,
/// vertically centered. Glyph pixels are written as solid white
/// with coverage as alpha — `image::imageops::overlay` later
/// composites the whole bar (including these alpha glyphs) over
/// the navicust background with the right blend, so we don't
/// need to do the bg/fg mix in here. Trims trailing chars that
/// would push the run past `max_width`.
fn rasterize_label(dst: &mut image::RgbaImage, text: &str, x0: i64, font_height: f32, max_width: u32) {
    let scale = PxScale::from(font_height);
    let font = LABEL_FONT.as_scaled(scale);
    // Center the line vertically: baseline sits at (h - line_h)/2
    // + ascent. ab_glyph reports a positive ascent and a negative
    // descent, so the line height collapses to ascent - descent
    // and the centered baseline is (h + ascent + descent) / 2.
    let h = dst.height() as f32;
    let baseline_y = (h + font.ascent() + font.descent()) / 2.0;

    let mut cursor_x = x0 as f32;
    let max_x = (x0 + max_width as i64) as f32;
    for ch in text.chars() {
        let glyph_id = font.glyph_id(ch);
        let advance = font.h_advance(glyph_id);
        if cursor_x + advance > max_x {
            break;
        }
        let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(cursor_x, baseline_y));
        if let Some(outlined) = font.font().outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|px, py, coverage| {
                let ix = bounds.min.x as i32 + px as i32;
                let iy = bounds.min.y as i32 + py as i32;
                if ix < 0 || iy < 0 || ix >= dst.width() as i32 || iy >= dst.height() as i32 {
                    return;
                }
                let a = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
                if a == 0 {
                    return;
                }
                let pixel = dst.get_pixel_mut(ix as u32, iy as u32);
                // Take the max alpha so glyphs that touch don't
                // darken the overlap by overwriting a higher
                // coverage with a lower one.
                if a > pixel.0[3] {
                    pixel.0 = [255, 255, 255, a];
                }
            });
        }
        cursor_x += advance;
    }
}

/// BN3-style color bar: White / Pink / Yellow / (extra from style).
/// Returns only the tile stripe; the wrapper in `render` pads
/// it to body width and overlays the style label on the left.
fn render_color_bar_bn3(extra: Option<NavicustPartColor>) -> image::RgbaImage {
    const TILE_WIDTH: f32 = SQUARE_SIZE / 4.0;
    let mut pixmap = tiny_skia::Pixmap::new(
        (TILE_WIDTH * 4.0 + BORDER_WIDTH) as u32,
        (SQUARE_SIZE / 2.0 + BORDER_WIDTH) as u32,
    )
    .unwrap();

    let bg_paint = solid_paint(BG_FILL_COLOR);
    let border_paint = solid_paint(BORDER_STROKE_COLOR);
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

    let root = tiny_skia::Transform::from_translate(BORDER_WIDTH / 2.0, BORDER_WIDTH / 2.0);

    let tiles = [
        Some(NavicustPartColor::White),
        Some(NavicustPartColor::Pink),
        Some(NavicustPartColor::Yellow),
        extra,
    ];
    for (i, color) in tiles.into_iter().enumerate() {
        let transform = root.pre_translate(i as f32 * TILE_WIDTH, 0.0);
        let paint = if let Some(color) = color {
            let (_, plus) = part_colors(color);
            solid_paint(plus)
        } else {
            bg_paint.clone()
        };
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
        pixmap.stroke_path(&path, &border_paint, &stroke, transform, None);
    }

    // Wrapper in `render` will pad this to body width + bake
    // the style label.
    image::ImageBuffer::from_raw(pixmap.width(), pixmap.height(), pixmap.take()).unwrap()
}

/// BN4/5/6 color bar: up to 4 non-bug colors on the left, optional bug
/// colors on the right (separated by a gap).
fn render_color_bar_bn456(
    body_width: u32,
    view: &dyn NavicustView,
    assets: &dyn Assets,
) -> image::RgbaImage {
    const TILE_WIDTH: f32 = SQUARE_SIZE * 3.0 / 4.0;
    let mut colors: Vec<NavicustPartColor> = Vec::new();
    for i in 0..view.count() {
        let Some(ncp) = view.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(ncp.id) else { continue };
        let Some(c) = info.color() else { continue };
        if !colors.contains(&c) {
            colors.push(c);
        }
    }

    let tile_count = std::cmp::max(4, colors.len()) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(
        TILE_WIDTH as u32 * tile_count + BORDER_WIDTH as u32 * 2,
        (SQUARE_SIZE / 2.0 + BORDER_WIDTH) as u32,
    )
    .unwrap();

    let bg_paint = solid_paint(BG_FILL_COLOR);
    let border_paint = solid_paint(BORDER_STROKE_COLOR);
    let stroke = tiny_skia::Stroke {
        line_cap: tiny_skia::LineCap::Square,
        width: BORDER_WIDTH,
        ..Default::default()
    };

    let nonbug = &colors[..colors.len().min(4)];
    let bug = colors.get(4..).unwrap_or(&[]);

    let root = tiny_skia::Transform::from_translate(BORDER_WIDTH / 2.0, BORDER_WIDTH / 2.0);

    let outline = {
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, TILE_WIDTH, SQUARE_SIZE / 2.0).unwrap());
        pb.finish().unwrap()
    };
    let inner = {
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
        let transform = root.pre_translate(i as f32 * TILE_WIDTH, 0.0);
        let paint = if let Some(c) = nonbug.get(i) {
            let (_, plus) = part_colors(c.clone());
            solid_paint(plus)
        } else {
            bg_paint.clone()
        };
        pixmap.fill_path(&inner, &paint, tiny_skia::FillRule::Winding, transform, None);
        pixmap.stroke_path(&outline, &border_paint, &stroke, transform, None);
    }
    for (i, c) in bug.iter().enumerate() {
        let transform = root.pre_translate((i + 4) as f32 * TILE_WIDTH + BORDER_WIDTH, 0.0);
        let (_, plus) = part_colors(c.clone());
        let paint = solid_paint(plus);
        pixmap.fill_path(&inner, &paint, tiny_skia::FillRule::Winding, transform, None);
    }

    let bar = image::ImageBuffer::from_raw(pixmap.width(), pixmap.height(), pixmap.take()).unwrap();
    pad_left(bar, body_width)
}

/// Pad an image to `target_w` by extending transparently on the left so
/// the bar's tiles align flush-right against the grid's right edge.
fn pad_left(img: image::RgbaImage, target_w: u32) -> image::RgbaImage {
    if img.width() >= target_w {
        return img;
    }
    let mut out = image::RgbaImage::new(target_w, img.height());
    let offset = (target_w - img.width()) as i64;
    image::imageops::overlay(&mut out, &img, offset, 0);
    out
}

pub fn render_grid(
    materialized: &MaterializedNavicust,
    layout: &NavicustLayout,
    view: &dyn NavicustView,
    assets: &dyn Assets,
) -> image::RgbaImage {
    let (height, width) = materialized.dim();
    let mut pixmap = tiny_skia::Pixmap::new(
        (width as f32 * SQUARE_SIZE + BORDER_WIDTH) as u32,
        (height as f32 * SQUARE_SIZE + BORDER_WIDTH) as u32,
    )
    .unwrap();

    let root_transform = tiny_skia::Transform::from_translate(BORDER_WIDTH / 2.0, BORDER_WIDTH / 2.0);

    let bg_fill_paint = solid_paint(BG_FILL_COLOR);
    let border_stroke_paint = solid_paint(BORDER_STROKE_COLOR);
    let stroke = tiny_skia::Stroke {
        line_cap: tiny_skia::LineCap::Square,
        width: BORDER_WIDTH,
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
            border_path: line_path(0.0, 0.0, SQUARE_SIZE, 0.0),
        },
        Neighbor {
            offset: [-1, 0],
            border_path: line_path(0.0, 0.0, 0.0, SQUARE_SIZE),
        },
        Neighbor {
            offset: [0, 1],
            border_path: line_path(0.0, SQUARE_SIZE, SQUARE_SIZE, SQUARE_SIZE),
        },
        Neighbor {
            offset: [1, 0],
            border_path: line_path(SQUARE_SIZE, 0.0, SQUARE_SIZE, SQUARE_SIZE),
        },
    ];

    // Pass 1: background squares (skipping the four oob corners when applicable).
    for y in 0..width {
        for x in 0..height {
            if layout.has_out_of_bounds
                && ((x == 0 && y == 0)
                    || (x == 0 && y == height - 1)
                    || (x == width - 1 && y == 0)
                    || (x == width - 1 && y == height - 1))
            {
                continue;
            }
            let transform = root_transform.pre_translate(x as f32 * SQUARE_SIZE, y as f32 * SQUARE_SIZE);
            pixmap.fill_path(&square_path, &bg_fill_paint, tiny_skia::FillRule::Winding, transform, None);
            pixmap.stroke_path(&square_path, &border_stroke_paint, &stroke, transform, None);
        }
    }

    // Pass 2: filled part squares.
    for (i, ncp_i) in materialized.iter().enumerate() {
        let x = i % width;
        let y = i / width;
        let Some(ncp_i) = ncp_i else { continue };
        let Some(ncp) = view.navicust_part(*ncp_i) else { continue };
        let Some(info) = assets.navicust_part(ncp.id) else { continue };
        let Some(color) = info.color() else { continue };

        let transform = root_transform.pre_translate(x as f32 * SQUARE_SIZE, y as f32 * SQUARE_SIZE);
        let (solid_color, plus_color) = part_colors(color);
        let fill_paint = solid_paint(solid_color);
        let stroke_paint = solid_paint(plus_color);

        pixmap.fill_path(&square_path, &fill_paint, tiny_skia::FillRule::Winding, transform, None);
        pixmap.stroke_path(&square_path, &stroke_paint, &stroke, transform, None);
        if !info.is_solid() {
            pixmap.stroke_path(&plus_path, &stroke_paint, &stroke, transform, None);
        }
    }

    // Pass 3: borders between different parts.
    for (i, ncp_i) in materialized.iter().enumerate() {
        let Some(ncp_i) = *ncp_i else { continue };
        let x = i % width;
        let y = i / width;
        let transform = root_transform.pre_translate(x as f32 * SQUARE_SIZE, y as f32 * SQUARE_SIZE);
        for n in neighbors.iter() {
            let nx = x as isize + n.offset[0];
            let ny = y as isize + n.offset[1];
            let mut should_stroke = nx < 0 || nx >= width as isize || ny < 0 || ny >= height as isize;
            if !should_stroke
                && materialized[[ny as usize, nx as usize]]
                    .map(|v| v != ncp_i)
                    .unwrap_or(true)
            {
                should_stroke = true;
            }
            if should_stroke {
                pixmap.stroke_path(&n.border_path, &border_stroke_paint, &stroke, transform, None);
            }
        }
    }

    // Pass 4: command line markers.
    let command_line_top = layout.command_line as f32 * SQUARE_SIZE;
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

    // Pass 5: out-of-bounds shading (the four 1x1 corner blocks + the
    // outer band, half-alpha black).
    if layout.has_out_of_bounds {
        let path = {
            let mut pb = tiny_skia::PathBuilder::new();
            let w = SQUARE_SIZE + BORDER_WIDTH;
            let h = (height - 2) as f32 * SQUARE_SIZE + BORDER_WIDTH;
            // left
            pb.push_rect(
                tiny_skia::Rect::from_xywh(-BORDER_WIDTH / 2.0, SQUARE_SIZE - BORDER_WIDTH / 2.0, w, h).unwrap(),
            );
            // right
            pb.push_rect(
                tiny_skia::Rect::from_xywh(
                    (width - 1) as f32 * SQUARE_SIZE - BORDER_WIDTH / 2.0,
                    SQUARE_SIZE - BORDER_WIDTH / 2.0,
                    w,
                    h,
                )
                .unwrap(),
            );
            // top
            pb.push_rect(
                tiny_skia::Rect::from_xywh(SQUARE_SIZE - BORDER_WIDTH / 2.0, -BORDER_WIDTH / 2.0, h, w).unwrap(),
            );
            // bottom
            pb.push_rect(
                tiny_skia::Rect::from_xywh(
                    SQUARE_SIZE - BORDER_WIDTH / 2.0,
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

    let w = pixmap.width();
    let h = pixmap.height();
    image::ImageBuffer::from_raw(w, h, pixmap.take()).unwrap()
}

fn solid_paint(rgba: [u8; 4]) -> tiny_skia::Paint<'static> {
    let mut p = tiny_skia::Paint::default();
    p.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    p
}

fn line_path(x1: f32, y1: f32, x2: f32, y2: f32) -> tiny_skia::Path {
    let mut pb = tiny_skia::PathBuilder::new();
    pb.move_to(x1, y1);
    pb.line_to(x2, y2);
    pb.finish().unwrap()
}
