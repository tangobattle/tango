//! NaviCust grid rendering. Ported from `tango/src/gui/save_view/navi_view/navicust_view.rs`,
//! omitting the color bar (which needed font rendering against the egui atlas).
//! Outputs an RGBA image we can hand to iced's image widget.

use tango_dataview::{
    navicust::MaterializedNavicust,
    rom::{Assets, NavicustLayout, NavicustPartColor},
    save::NavicustView,
};

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

/// Background fill + color bar + body. Matches the egui app's
/// `render_navicust`, minus the style name text overlaid on the bar.
pub fn render(
    materialized: &MaterializedNavicust,
    layout: &NavicustLayout,
    view: &dyn NavicustView,
    assets: &dyn Assets,
) -> image::RgbaImage {
    let body = render_grid(materialized, layout, view, assets);

    let color_bar = if let Some(style_id) = view.style() {
        let extra = assets.style(style_id).and_then(|s| s.extra_ncp_color());
        render_color_bar_bn3(body.width(), extra)
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

/// BN3-style color bar: White / Pink / Yellow / (extra from style).
fn render_color_bar_bn3(body_width: u32, extra: Option<NavicustPartColor>) -> image::RgbaImage {
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

    // Pad bar to body width so it lines up with the grid.
    let bar = image::ImageBuffer::from_raw(pixmap.width(), pixmap.height(), pixmap.take()).unwrap();
    pad_left(bar, body_width)
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
