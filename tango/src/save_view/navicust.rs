use super::*;
use sweeten::widget::{column, row};

/// The palette thumbnail for part `id` at orientation `(rot, compressed)`.
/// The default orientation reuses the icon baked once at load; a rotated /
/// uncompressed shape is drawn live by a small canvas ([`PartThumb`]) so
/// we never re-bake an image (which would mint a fresh texture id every
/// frame). `dim` fades it for at-cap rows. `None` for an empty shape.
fn part_thumb<'a>(loaded: &'a Loaded, id: usize, rot: u8, compressed: bool, dim: bool) -> Option<Element<'a, Action>> {
    if rot == 0 && compressed {
        let (w, h, handle) = loaded.navicust_part_icons.get(id)?.as_ref()?;
        return Some(
            Image::new(handle.clone())
                .width(Length::Fixed(*w as f32))
                .height(Length::Fixed(*h as f32))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::None)
                .opacity(if dim { 0.35 } else { 1.0 })
                .into(),
        );
    }
    let info = loaded.assets.navicust_part(id)?;
    let color = info.color()?;
    let bitmap = info
        .compressed_bitmap()
        .filter(|_| compressed)
        .unwrap_or_else(|| info.uncompressed_bitmap());
    let rotated = crate::navicust::rotate_bitmap(&bitmap, rot);
    crate::navicust_editor::PartThumb::new(&rotated, color, info.is_solid(), dim).map(|t| t.view())
}

/// The navicust editor: an interactive grid (left) + a part palette
/// (right), mirroring [`render_folder_edit`]'s two-pane layout — the grid
/// pane shrinks to the grid so the palette gets the rest of the width. The
/// grid is drawn live by [`crate::navicust_editor::EditorGrid`], which
/// shares the decoration-drawing routine ([`crate::navicust::paint`]) with
/// the read-only viewer and the clipboard image, and ghosts the held part.
/// Each palette row carries its own rotate / (de)compress buttons that set
/// the orientation the part is picked up in.
pub(super) fn render_navicust_edit<'a>(lang: &'a LanguageIdentifier, loaded: &'a Loaded, state: &'a State) -> Element<'a, Action> {
    use crate::widgets;
    // Only reached while editing, so the EditState is present.
    let Some(edit) = state.editing.as_ref() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let Some(tango_dataview::save::NaviView::Navicust(v)) = loaded.save.view_navi() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let size = v.size();
    let (cols, rows) = (size[0], size[1]);
    // BN4/5/6 (the only editable navicust games) always publish a layout.
    let Some(layout) = assets.navicust_layout() else {
        return placeholder(t!(lang, "save-empty"));
    };

    // Live grid recomputed from the part slots (NOT the WRAM cache), so
    // staged edits show immediately. `materialize` takes `[rows, cols]`.
    let materialized = tango_dataview::navicust::materialize(v.as_ref(), [rows, cols], assets);
    let model = crate::navicust::build_model(&materialized, &layout, v.as_ref(), assets);
    let installed = (0..v.count()).filter(|&i| v.navicust_part(i).is_some()).count();
    // Cell → installed-part slot, captured before `model` is moved into the
    // grid, to drive the per-cell hover popover overlay below.
    let occupancy = model.occupancy.clone();

    // Held-part ghost data, resolved from the ROM.
    let held = edit.held_part.and_then(|hp| {
        let info = assets.navicust_part(hp.id)?;
        let color = info.color()?;
        let bitmap = info
            .compressed_bitmap()
            .filter(|_| hp.compressed)
            .unwrap_or_else(|| info.uncompressed_bitmap());
        let (solid, plus) = crate::navicust::part_colors(color);
        Some(crate::navicust_editor::Held {
            cells: crate::navicust_editor::rotated_offsets(&bitmap, hp.rot),
            grab: (hp.grab_row as isize, hp.grab_col as isize),
            solid,
            plus,
            is_solid: info.is_solid(),
        })
    });

    // Editor grid geometry (must match `EditorGrid::new`) so the hover
    // popover overlay's cells line up with the painted squares.
    let g = crate::navicust::geometry(cols, rows);
    let scale = crate::navicust::display_scale(crate::navicust_editor::DISPLAY_W);
    let cell = crate::navicust::SQUARE_SIZE * scale;
    let origin_x = (g.body_origin_x + crate::navicust::BORDER_WIDTH / 2.0) * scale;
    let origin_y = (g.body_origin_y + crate::navicust::BORDER_WIDTH / 2.0) * scale;
    let grid_w = g.total_w * scale;
    let grid_h = g.total_h * scale;

    let canvas_el: Element<'a, Action> = crate::navicust_editor::EditorGrid::new(model, held).view();

    // Per-cell hover popover (part name + description), mirroring the
    // read-only viewer: a fixed grid of cell-sized spaces with each covered
    // cell tooltip-wrapped. Stacked *over* the canvas: the cells report
    // `Interaction::None`, so iced's Stack doesn't levitate the cursor away
    // from the canvas beneath (its clicks / scroll / ghost still work, and
    // its Pointer/Crosshair cursor still wins), while the tooltips get the
    // real cursor and fire. Beneath the canvas they wouldn't — the canvas's
    // non-None interaction levitates the cursor off any lower layer.
    let mut overlay_col = column![Space::new().height(Length::Fixed(origin_y))];
    for r in 0..rows {
        let mut cell_row = row![Space::new().width(Length::Fixed(origin_x))];
        for c in 0..cols {
            let info = occupancy
                .get(r * cols + c)
                .copied()
                .flatten()
                .and_then(|slot| v.navicust_part(slot))
                .and_then(|p| assets.navicust_part(p.id));
            let cell_el: Element<'a, Action> = if let Some(info) = info {
                let name = info.name().unwrap_or_else(|| "?".to_string());
                let mut tip_col = column![text(name).size(TEXT_BODY)].spacing(2);
                if let Some(desc) = info.description() {
                    tip_col = tip_col.push(text(desc).size(TEXT_CAPTION));
                }
                let tip = container(tip_col).padding(8).style(tooltip_style);
                let space = Space::new().width(Length::Fixed(cell)).height(Length::Fixed(cell));
                tooltip(space, tip, tooltip::Position::FollowCursor).gap(12).into()
            } else {
                Space::new()
                    .width(Length::Fixed(cell))
                    .height(Length::Fixed(cell))
                    .into()
            };
            cell_row = cell_row.push(cell_el);
        }
        overlay_col = overlay_col.push(cell_row);
    }
    let canvas_el: Element<'a, Action> = stack![canvas_el, overlay_col]
        .width(Length::Fixed(grid_w))
        .height(Length::Fixed(grid_h))
        .into();

    // Installed copies per part id — palette entries for parts already at
    // the per-part cap are shown disabled (not selectable).
    let mut installed_counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for i in 0..v.count() {
        if let Some(p) = v.navicust_part(i) {
            *installed_counts.entry(p.id).or_insert(0) += 1;
        }
    }

    // ----- Left pane: grid + rotate/compress controls -----
    let count = text(t!(lang, "navicust-edit-count", count = installed as i64))
        .size(TEXT_CAPTION)
        .style(muted_text_style);
    let grid_header = editor_header(
        lang,
        t!(lang, "navicust-edit-grid"),
        vec![count.into()],
        Action::ClearNavicust,
    );

    let held_opt = edit.held_part;

    // ----- Part palette (shown below the grid, like the read-only view) -----
    // Rows run flush to the pane sides (no side inset); shares only its
    // row spacing with the patches / replays lists.
    let mut palette = column![].spacing(2).padding(0).width(Fill);
    for (row_idx, (id, name, description)) in sorted_navicust_parts(loaded, state.navicust_sort, &edit.navicust_filter)
        .into_iter()
        .enumerate()
    {
        // Parts already at the per-part copy cap are greyed out + not
        // selectable.
        let at_cap = installed_counts.get(&id).copied().unwrap_or(0) >= crate::navicust_editor::MAX_COPIES_PER_PART;
        // Orientation shown in (and picked up from) the picker.
        let (rot, compressed) = edit.orient_of(id);
        // Shape thumbnail at the part's current picker orientation, shown
        // at the baked pixel size (1:1) so the 1px lines stay crisp; every
        // part shares the same n×n grid so rows align. Dimmed when at cap.
        let icon_el: Element<'a, Action> = part_thumb(loaded, id, rot, compressed, at_cap).unwrap_or_else(|| {
            Space::new()
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
                .into()
        });
        let selected = held_opt.map_or(false, |h| h.id == id);
        let name_text = if at_cap {
            text(name).size(TEXT_BODY).style(muted_text_style)
        } else {
            text(name).size(TEXT_BODY)
        };
        let mut info_col = column![name_text].spacing(1);
        if let Some(desc) = description.filter(|d| !d.trim().is_empty()) {
            // On the selected (held) row, drop the muted wash so the
            // description inherits the gold plate's ink — every line
            // reads in the title's color, same as the replay list.
            info_col = info_col.push(text(desc).size(TEXT_CAPTION).style(move |theme: &iced::Theme| {
                if selected {
                    iced::widget::text::Style { color: None }
                } else {
                    muted_text_style(theme)
                }
            }));
        }
        // Per-part orientation controls: rotate, and (de)compress. They
        // edit this part's picker entry — including the thumbnail beside
        // them and the orientation it's picked up in. The compress button
        // names the action it performs (Uncompress when compressed, else
        // Compress). They're nested inside the row's pick-up button (iced
        // forwards clicks to these inner buttons first), so they live on the
        // menu item itself rather than floating beside it.
        let rotate_btn = widgets::icon_button(
            lucide_icons::Icon::RotateCw,
            t!(lang, "navicust-edit-rotate"),
            Action::RotatePart { id },
            [6.0, 8.0],
        );
        let (compress_icon, compress_label) = if compressed {
            (lucide_icons::Icon::Expand, t!(lang, "navicust-edit-uncompress"))
        } else {
            (lucide_icons::Icon::Shrink, t!(lang, "navicust-edit-compress"))
        };
        // A part whose compressed and uncompressed shapes are identical can't
        // be (de)compressed — render the button disabled rather than letting
        // it toggle a flag with no visible effect.
        let compressible = loaded
            .assets
            .navicust_part(id)
            .and_then(|info| info.compressed_bitmap().map(|bmp| bmp != info.uncompressed_bitmap()))
            .unwrap_or(false);
        let compress_btn = widgets::icon_button_maybe(
            compress_icon,
            compress_label,
            compressible.then_some(Action::ToggleCompressPart { id }),
            [6.0, 8.0],
        );
        let controls = column![rotate_btn, compress_btn].spacing(4).align_x(Alignment::Center);
        let content = row![icon_el, info_col, Space::new().width(Fill), controls]
            .spacing(8)
            .align_y(Alignment::Center);
        let mut pick = button(content)
            .padding(style::ROW_PADDING)
            .width(Fill)
            .style(widgets::list_item(selected, row_idx));
        if !at_cap {
            pick = pick.on_press(Action::PickUpPalettePart { id });
        }
        palette = palette.push(pick);
    }
    let parts_header = library_header(
        lang,
        t!(lang, "navicust-edit-search"),
        &edit.navicust_filter,
        Action::NavicustFilterChanged,
        &NavicustSort::ALL,
        state.navicust_sort,
        NavicustSort::label,
        Action::NavicustSortChanged,
    );

    // Left pane: mirrors the read-only Navi view — the grid with the
    // installed ("picked") parts listed below it — and fills/expands to
    // its half of the tab, with the grid + parts centered inside.
    let mut grid_inner = column![container(canvas_el).center_x(Fill)]
        .spacing(8)
        .align_x(Alignment::Center)
        .padding([4, 8]);
    if let Some(parts) = navicust_installed_parts::<Action>(loaded, v.as_ref()) {
        grid_inner = grid_inner.push(parts);
    }

    // Grid on the left, the editing palette filling the remaining width.
    editor_panes(editor_pane(grid_header, grid_inner), editor_pane(parts_header, palette))
}

// ---------- Navi ----------

pub(super) fn render_navi<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(navi_view) = loaded.save.view_navi() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    match navi_view {
        tango_dataview::save::NaviView::LinkNavi(v) => {
            let navi_id = v.navi();
            let name = assets
                .navi(navi_id)
                .and_then(|n| n.name())
                .unwrap_or_else(|| format!("Navi #{navi_id}"));
            // Plate/glow tint: the emblem's own signature color, with a
            // neutral slate fallback for monochrome emblems.
            let accent = loaded
                .navi_accents
                .get(&navi_id)
                .copied()
                .unwrap_or(iced::Color::from_rgb8(0x6b, 0x7a, 0x99));

            // Emblem at an integer multiple of its 15px crop so the
            // nearest-neighbor upscale lands on even pixels.
            let emblem: Element<'static, M> = loaded
                .navi_emblems
                .get(&navi_id)
                .cloned()
                .map(|h| {
                    Image::new(h)
                        .width(Length::Fixed(90.0))
                        .height(Length::Fixed(90.0))
                        .filter_method(iced_image::FilterMethod::Nearest)
                        .content_fit(ContentFit::Contain)
                        .into()
                })
                .unwrap_or_else(|| {
                    Space::new()
                        .width(Length::Fixed(90.0))
                        .height(Length::Fixed(90.0))
                        .into()
                });

            // Circular plate behind the emblem: accent-tinted fill, a
            // ring a shade brighter, and an accent glow lifting it off
            // the pane.
            let plate: Element<'static, M> = container(emblem)
                .width(Length::Fixed(140.0))
                .height(Length::Fixed(140.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(move |theme: &iced::Theme| {
                    let bg = theme.palette().background;
                    container::Style {
                        background: Some(iced::Background::Color(crate::widgets::mix(bg, accent, 0.22))),
                        border: iced::Border {
                            radius: 70.0.into(),
                            width: 2.0,
                            color: iced::Color { a: 0.8, ..accent },
                        },
                        shadow: iced::Shadow {
                            color: iced::Color { a: 0.45, ..accent },
                            offset: iced::Vector::new(0.0, 0.0),
                            blur_radius: 26.0,
                        },
                        ..Default::default()
                    }
                })
                .into();

            let card = column![
                plate,
                column![
                    text(name).size(TEXT_DISPLAY),
                    text(t!(lang, "navi-link-navi"))
                        .size(TEXT_CAPTION)
                        .style(muted_text_style),
                ]
                .spacing(2)
                .align_x(Alignment::Center),
            ]
            .spacing(16)
            .align_x(Alignment::Center);

            // The pane itself picks up a whisper of the accent, fading
            // back to the standard plate color toward the bottom.
            container(card)
                .width(Fill)
                .align_x(Alignment::Center)
                .padding([28.0, crate::style::PANE_PADDING])
                .style(move |theme: &iced::Theme| {
                    let mut s = crate::widgets::pane(theme);
                    if let Some(iced::Background::Color(plate_color)) = s.background {
                        // Stop 0 sits at the bottom for a 0-radian linear
                        // gradient, so the accent goes on stop 1 — the
                        // tint halos the plate at the top of the card.
                        s.background = Some(iced::Background::Gradient(iced::Gradient::Linear(
                            iced::gradient::Linear::new(0.0)
                                .add_stop(0.0, plate_color)
                                .add_stop(1.0, crate::widgets::mix(plate_color, accent, 0.10)),
                        )));
                    }
                    s
                })
                .into()
        }
        tango_dataview::save::NaviView::Navicust(v) => render_navicust(lang, loaded, v.as_ref()),
    }
}

/// The installed-parts badge strip shown under the grid: two columns
/// (solid parts | plus parts), each badge colored by its NCP color with a
/// description tooltip. Reads the live view, so it reflects staged edits.
/// `None` when nothing is installed. Shared by the read-only Navi view and
/// the editor's grid pane.
fn navicust_installed_parts<M: 'static>(
    loaded: &Loaded,
    v: &dyn tango_dataview::save::NavicustView,
) -> Option<Element<'static, M>> {
    let assets = loaded.assets.as_ref();
    let mut solid_col = column![].spacing(4);
    let mut plus_col = column![].spacing(4);
    let mut any = false;
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let part_name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        let description = info.description();
        let is_solid = info.is_solid();
        let (solid_color, plus_color) = info.color().map(ncp_colors).unwrap_or((
            iced::Color::from_rgb8(0xbd, 0xbd, 0xbd),
            iced::Color::from_rgb8(0x88, 0x88, 0x88),
        ));
        let bg = if is_solid { solid_color } else { plus_color };
        let badge_el = colored_badge_sized(part_name, bg, iced::Color::BLACK, TEXT_BODY, [3.0, 8.0]);
        let badge_el: Element<'static, M> = if let Some(desc) = description {
            tooltip(
                badge_el,
                container(text(desc).size(TEXT_CAPTION)).padding(8).style(tooltip_style),
                tooltip::Position::FollowCursor,
            )
            .gap(8)
            .into()
        } else {
            badge_el
        };
        any = true;
        if is_solid {
            solid_col = solid_col.push(badge_el);
        } else {
            plus_col = plus_col.push(badge_el);
        }
    }
    any.then(|| row![solid_col, plus_col].spacing(12).into())
}

/// The viewer's installed-parts panel, shown beside the grid: one row
/// per part with its shape thumbnail (bounding-box crop, native pixel
/// scale), its name badge, and its description inline. Solid parts
/// first, then plus parts, keeping slot order within each group — the
/// same ordering the badge strip used. `None` when nothing is
/// installed.
fn navicust_parts_panel<M: 'static>(
    loaded: &Loaded,
    v: &dyn tango_dataview::save::NavicustView,
) -> Option<Element<'static, M>> {
    let assets = loaded.assets.as_ref();
    let mut solid_rows: Vec<Element<'static, M>> = vec![];
    let mut plus_rows: Vec<Element<'static, M>> = vec![];
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let part_name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        let is_solid = info.is_solid();
        let (solid_color, plus_color) = info.color().map(ncp_colors).unwrap_or((
            iced::Color::from_rgb8(0xbd, 0xbd, 0xbd),
            iced::Color::from_rgb8(0x88, 0x88, 0x88),
        ));
        let bg = if is_solid { solid_color } else { plus_color };

        // Shape thumb at its native baked scale (8 px per cell), centered
        // in a fixed box so the name column lines up across rows. The
        // largest shapes (5+ cells on a side) scale down to fit.
        const THUMB_BOX: f32 = 40.0;
        let thumb: Element<'static, M> = loaded
            .navicust_part_icons_cropped
            .get(part.id)
            .and_then(|o| o.clone())
            .map(|(w, h, handle)| {
                Image::new(handle)
                    .width(Length::Fixed((w as f32).min(THUMB_BOX)))
                    .height(Length::Fixed((h as f32).min(THUMB_BOX)))
                    .filter_method(iced_image::FilterMethod::Nearest)
                    .content_fit(ContentFit::Contain)
                    .into()
            })
            .unwrap_or_else(|| Space::new().into());
        let thumb_box: Element<'static, M> = container(thumb)
            .width(Length::Fixed(THUMB_BOX))
            .height(Length::Fixed(THUMB_BOX))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into();

        let mut name_col = column![colored_badge_sized::<M>(
            part_name,
            bg,
            iced::Color::BLACK,
            TEXT_BODY,
            [3.0, 8.0]
        )]
        .spacing(3)
        .align_x(Alignment::Start);
        if let Some(desc) = info.description() {
            // ROM descriptions keep the game's own textbox line breaks —
            // they're authored to wrap there, so the text shrink-wraps to
            // its natural width.
            name_col = name_col.push(text(desc).size(TEXT_CAPTION).style(muted_text_style));
        }

        let row_el: Element<'static, M> = row![thumb_box, name_col].spacing(10).align_y(Alignment::Center).into();
        if is_solid {
            solid_rows.push(row_el);
        } else {
            plus_rows.push(row_el);
        }
    }
    if solid_rows.is_empty() && plus_rows.is_empty() {
        return None;
    }
    // Two top-aligned columns, like the badge strip this replaces:
    // solid parts on the left, plus parts on the right.
    let mut solid_col = column![].spacing(6);
    for r in solid_rows {
        solid_col = solid_col.push(r);
    }
    let mut plus_col = column![].spacing(6);
    for r in plus_rows {
        plus_col = plus_col.push(r);
    }
    Some(row![solid_col, plus_col].spacing(20).into())
}

fn render_navicust<M: 'static>(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
    v: &dyn tango_dataview::save::NavicustView,
) -> Element<'static, M> {
    let assets = loaded.assets.as_ref();
    let [cols, rows_n] = v.size();

    // Big rendered grid (tiny-skia, cached at load time). Scale down to
    // ~440 px wide if larger (5×5 grids render around 360 wide native;
    // bigger grids get scaled). Wrapped in mouse_area so hovering over
    // Per-cell tooltip overlay: render the image as one layer of a
    // Stack and a column-of-rows-of-cell-sized empty widgets as the
    // second layer. Each cell that's covered by an installed part gets
    // its own tooltip wrapper, so iced's tooltip widget manages the
    // hover state internally — no NavicustHover message round-trip
    // needed.
    let grid_el: Element<'static, M> = match loaded.navicust_render.as_ref() {
        Some(nc) => {
            // `source_w/h` are now in DISPLAY coords (see selection.rs);
            // the underlying Handle is 2× that, and iced linear-
            // downsamples it for HiDPI crispness.
            let dw = nc.source_w as f32;
            let dh = nc.source_h as f32;
            let body_x = nc.body_origin_x;
            let body_y = nc.body_origin_y;
            let cell_size = nc.cell_size;
            let g_cols = nc.cols;
            let g_rows = nc.rows;

            let image: Element<'static, M> = Image::new(nc.handle.clone())
                .width(Length::Fixed(dw))
                .height(Length::Fixed(dh))
                // Handle is 2× source for HiDPI (see selection.rs
                // build_navicust_render). On a 2× display iced
                // presents at native device pixels = perfect; on
                // a 1× display iced linear-downsamples 2:1.
                .filter_method(iced_image::FilterMethod::Linear)
                .content_fit(ContentFit::Contain)
                .into();

            // Build the overlay: a fixed-size column of fixed-size rows
            // matching the grid. Each cell is either a no-op Space or
            // a tooltip-wrapped Space carrying the part's name + desc.
            let mut overlay_col = column![Space::new().height(Length::Fixed(body_y))];
            for row_idx in 0..g_rows {
                let mut cell_row = row![Space::new().width(Length::Fixed(body_x))];
                for col_idx in 0..g_cols {
                    let cell_idx = nc.cell_part_idx.get(row_idx * g_cols + col_idx).copied().flatten();
                    let info = cell_idx
                        .and_then(|pi| v.navicust_part(pi))
                        .and_then(|p| assets.navicust_part(p.id));
                    let cell: Element<'static, M> = if let Some(info) = info {
                        let name = info.name().unwrap_or_else(|| "?".to_string());
                        let mut tip_col = column![text(name).size(TEXT_BODY)].spacing(2);
                        if let Some(desc) = info.description() {
                            tip_col = tip_col.push(text(desc).size(TEXT_CAPTION));
                        }
                        let tip = container(tip_col).padding(8).style(tooltip_style);
                        let space = Space::new()
                            .width(Length::Fixed(cell_size))
                            .height(Length::Fixed(cell_size));
                        tooltip(space, tip, tooltip::Position::FollowCursor).gap(12).into()
                    } else {
                        Space::new()
                            .width(Length::Fixed(cell_size))
                            .height(Length::Fixed(cell_size))
                            .into()
                    };
                    cell_row = cell_row.push(cell);
                }
                overlay_col = overlay_col.push(cell_row);
            }

            // Top layer: outline the block under the cursor. It never
            // captures events, so the tooltip layer beneath still fires.
            let hover: Element<'static, M> = crate::navicust_editor::HoverOutline {
                cols: g_cols,
                rows: g_rows,
                origin_x: body_x,
                origin_y: body_y,
                cell: cell_size,
                width: dw,
                height: dh,
                occupancy: nc.cell_part_idx.clone(),
            }
            .view::<M>();

            let stacked = stack![image, overlay_col, hover]
                .width(Length::Fixed(dw))
                .height(Length::Fixed(dh));
            // Flush against the pane — no shadow, no extra padding.
            // The image's corners are pre-masked in selection.rs to
            // match the pane's rounded corners. No Fill / centering
            // here either: that would propagate up and stretch the
            // whole pane across the tab.
            stacked.into()
        }
        None => text(t!(lang, "navicust-grid-size", cols = cols as i64, rows = rows_n as i64))
            .size(TEXT_CAPTION)
            .into(),
    };

    // Single pane sized to its contents — no "(none installed)"
    // fallback; an empty navicust shows just the rounded image with
    // pane padding around it. The installed-parts panel sits beside
    // the grid (the tab is much wider than the grid), top-aligned
    // with the grid body (the small padding eats the gap the image's
    // baked-in margin leaves above the color bar). No Fill anywhere:
    // that would stretch the pane across the tab.
    let mut content = row![grid_el].spacing(20).align_y(Alignment::Start);
    if let Some(parts) = navicust_parts_panel::<M>(loaded, v) {
        content = content.push(container(parts).padding([14.0, 0.0]));
    }

    let _ = (cols, rows_n);
    container(content)
        .padding(crate::style::PANE_PADDING)
        .style(crate::widgets::pane)
        .into()
}
