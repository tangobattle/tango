//! The NaviCust tab (the desktop's `save_view/navicust/mod.rs`): the
//! grid (SVG, see [`grid`]), a hover outline + name/description popover
//! over installed parts, the BN3 style label on the color bar, and the
//! installed-parts panel beside the grid.

use dioxus::prelude::*;
use tango_dataview::rom::NavicustPartColor;
use unic_langid::LanguageIdentifier;

use super::{placeholder, Loaded, SaveHandle};
use crate::t;

pub mod grid;

/// Visual width the reference (7-wide) grid is drawn at, in CSS px —
/// matches the desktop editor's `DISPLAY_W`, which the viewer shares.
pub const DISPLAY_W: f32 = 360.0;

/// Solid + plus colors for an NCP color as CSS strings, for the
/// installed-parts badges.
fn ncp_css_colors(color: Option<NavicustPartColor>) -> (String, String) {
    match color {
        Some(c) => {
            let (solid, plus) = grid::part_colors(c);
            (
                format!("rgb({},{},{})", solid[0], solid[1], solid[2]),
                format!("rgb({},{},{})", plus[0], plus[1], plus[2]),
            )
        }
        None => ("rgb(189,189,189)".to_string(), "rgb(136,136,136)".to_string()),
    }
}

/// The hovered installed part's slot (drives the white block outline in
/// the grid; changes per cell crossing) and the cursor position (drives
/// only the follow-cursor popover, so per-pixel moves don't re-render the
/// whole save view).
pub(super) static NCP_HOVER_SLOT: GlobalSignal<Option<usize>> = Signal::global(|| None);
pub(super) static NCP_HOVER_POS: GlobalSignal<(f64, f64)> = Signal::global(|| (0.0, 0.0));

/// The NaviCust tab: the equipped navi's customizer grid.
pub(super) fn render_navicust_tab(lang: &LanguageIdentifier, loaded: &Loaded) -> Element {
    let Some(v) = loaded.save.view_navicust() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let [cols, rows_n] = v.size();

    let Some(layout) = assets.navicust_layout() else {
        return rsx! {
            div { class: "pane ncp-pane",
                span { class: "sub", {t!(lang, "navicust-grid-size", cols = cols as i64, rows = rows_n as i64)} }
            }
        };
    };

    // Read-only view renders the WRAM-materialized cache, like the desktop.
    let materialized = v.materialized();
    let model = grid::build_model(&materialized, &layout, v.as_ref(), assets);
    let hover_slot = *NCP_HOVER_SLOT.read();
    let svg = grid::grid_svg(&model, None, hover_slot);

    // Geometry for the hover overlay + style label, in display pixels.
    let g = grid::geometry(model.cols, model.rows);
    let scale = grid::display_scale(DISPLAY_W);
    let (dw, dh) = (g.total_w * scale, g.total_h * scale);
    let origin_x = (g.body_origin_x + grid::BORDER_WIDTH / 2.0) * scale;
    let origin_y = (g.body_origin_y + grid::BORDER_WIDTH / 2.0) * scale;
    let cell = grid::SQUARE_SIZE * scale;
    let (m_cols, m_rows) = (model.cols, model.rows);
    let occupancy = model.occupancy.clone();

    // BN3: the style name rides the color bar's left edge.
    let style_label = v
        .style()
        .and_then(|sid| assets.style(sid).and_then(|s| s.name()))
        .map(|name| {
            let left = (grid::PADDING_H + grid::BORDER_WIDTH + 4.0) * scale;
            let top = grid::PADDING_V * scale;
            let h = g.bar_h * scale;
            let max_w = (g.body_w - (grid::SQUARE_SIZE + grid::BORDER_WIDTH) - (grid::BORDER_WIDTH + 4.0) * 2.0) * scale;
            let font = h * 0.72;
            rsx! {
                div {
                    class: "ncp-style-label",
                    style: "left:{left}px;top:{top}px;height:{h}px;max-width:{max_w}px;font-size:{font}px",
                    "{name}"
                }
            }
        });

    let grid_el = rsx! {
        div { class: "ncp-wrap", style: "width:{dw}px;height:{dh}px",
            {svg}
            {style_label}
            div {
                class: "ncp-overlay",
                onmousemove: move |evt| {
                    let p = evt.element_coordinates();
                    let col = ((p.x - origin_x as f64) / cell as f64).floor();
                    let row = ((p.y - origin_y as f64) / cell as f64).floor();
                    let slot = (col >= 0.0 && row >= 0.0 && (col as usize) < m_cols && (row as usize) < m_rows)
                        .then(|| occupancy.get(row as usize * m_cols + col as usize).copied().flatten())
                        .flatten();
                    let c = evt.client_coordinates();
                    *NCP_HOVER_POS.write() = (c.x, c.y);
                    if *NCP_HOVER_SLOT.peek() != slot {
                        *NCP_HOVER_SLOT.write() = slot;
                    }
                },
                onmouseleave: move |_| {
                    if NCP_HOVER_SLOT.peek().is_some() {
                        *NCP_HOVER_SLOT.write() = None;
                    }
                },
            }
        }
    };

    // The installed-parts panel beside the grid.
    let parts = navicust_parts_panel(loaded, v.as_ref());

    rsx! {
        div { class: "pane ncp-pane",
            {grid_el}
            if let Some(parts) = parts {
                div { class: "ncp-parts", {parts} }
            }
        }
    }
}

/// The follow-cursor popover for the hovered installed part: name +
/// description on the standard tooltip chrome. Mounted once by the save
/// view; renders nothing while no part is hovered.
#[component]
pub(super) fn NcpPopover(handle: SaveHandle) -> Element {
    let slot = *NCP_HOVER_SLOT.read();
    let (x, y) = *NCP_HOVER_POS.read();
    let Some(slot) = slot else { return rsx! {} };
    let l = handle.0.borrow();
    let info = l
        .save
        .view_navicust()
        .and_then(|v| v.navicust_part(slot))
        .and_then(|p| l.assets.navicust_part(p.id));
    let Some(info) = info else { return rsx! {} };
    let name = info.name().unwrap_or_else(|| "?".to_string());
    let description = info.description();
    let flip = web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .is_some_and(|w| x > w - 320.0);
    rsx! {
        div {
            class: if flip { "chip-pop flip" } else { "chip-pop" },
            style: "left:{x}px; top:{y}px",
            span { "{name}" }
            if let Some(desc) = description {
                p { "{desc}" }
            }
        }
    }
}

/// The viewer's installed-parts panel, shown beside the grid: one row per
/// part with its shape thumbnail (bounding-box crop at the part's actual
/// rotation + compression), its name badge, and its description. Solid
/// parts in the left column, plus parts in the right, keeping slot order
/// within each group. `None` when nothing is installed.
fn navicust_parts_panel(loaded: &Loaded, v: &dyn tango_dataview::save::NavicustView) -> Option<Element> {
    let assets = loaded.assets.as_ref();
    let mut solid_rows: Vec<Element> = vec![];
    let mut plus_rows: Vec<Element> = vec![];
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let part_name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        let is_solid = info.is_solid();
        let (solid_color, plus_color) = ncp_css_colors(info.color());
        let bg = if is_solid { solid_color } else { plus_color };

        // Thumb at the slot's actual orientation, cropped to its bounding
        // box, centered in a fixed 40px box so the name column lines up.
        let thumb = info.color().and_then(|color| {
            let bitmap = info
                .compressed_bitmap()
                .filter(|_| part.compressed)
                .unwrap_or_else(|| info.uncompressed_bitmap());
            let rotated = grid::rotate_bitmap(&bitmap, part.rot);
            grid::part_thumb_svg(&rotated, color, is_solid, true, false)
        });

        let row_el = rsx! {
            div { class: "ncp-part-row",
                span { class: "thumb-box", {thumb} }
                div { class: "part-info",
                    span { class: "part-badge", style: "background:{bg}", "{part_name}" }
                    if let Some(desc) = info.description() {
                        p { class: "sub", "{desc}" }
                    }
                }
            }
        };
        if is_solid {
            solid_rows.push(row_el);
        } else {
            plus_rows.push(row_el);
        }
    }
    if solid_rows.is_empty() && plus_rows.is_empty() {
        return None;
    }
    Some(rsx! {
        div { class: "ncp-col",
            for r in solid_rows {
                {r}
            }
        }
        div { class: "ncp-col",
            for r in plus_rows {
                {r}
            }
        }
    })
}

/// The NaviCust tab as text: style name first (BN3 only), then two TSV
/// columns — solid parts left, plus parts right, lined up row-by-row.
pub(crate) fn navicust_as_text(loaded: &Loaded) -> Option<String> {
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
