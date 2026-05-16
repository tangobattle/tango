use crate::i18n::t;
use crate::selection::Loaded;
use crate::Message;
use iced::widget::{
    checkbox, column, container, horizontal_rule, image as iced_image, row, scrollable, text, tooltip, Image, Space,
};
use iced::{Alignment, ContentFit, Element, Fill, Length};
use tango_dataview::rom::NavicustPartColor;
use tango_dataview::save::Save;
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Cover,
    Navi,
    Folder,
    PatchCards,
    AutoBattleData,
}

#[derive(Default, Clone, Copy)]
pub struct RenderOpts {
    pub folder_grouped: bool,
}

pub fn available_tabs(save: &dyn Save, streamer_mode: bool) -> Vec<Tab> {
    let mut tabs = vec![];
    if streamer_mode {
        tabs.push(Tab::Cover);
    }
    if save.view_navi().is_some() {
        tabs.push(Tab::Navi);
    }
    if save.view_chips().is_some() {
        tabs.push(Tab::Folder);
    }
    if save.view_patch_cards().is_some() {
        tabs.push(Tab::PatchCards);
    }
    if save.view_auto_battle_data().is_some() {
        tabs.push(Tab::AutoBattleData);
    }
    tabs
}

pub fn tab_key(tab: Tab) -> &'static str {
    match tab {
        Tab::Cover => "save-tab-cover",
        Tab::Navi => "save-tab-navi",
        Tab::Folder => "save-tab-folder",
        Tab::PatchCards => "save-tab-patch-cards",
        Tab::AutoBattleData => "save-tab-auto-battle-data",
    }
}

pub fn render(
    lang: &LanguageIdentifier,
    tab: Tab,
    loaded: &Loaded,
    opts: RenderOpts,
) -> Element<'static, Message> {
    match tab {
        Tab::Cover => render_cover(lang),
        Tab::Navi => render_navi(lang, loaded),
        Tab::Folder => render_folder(lang, loaded, opts.folder_grouped),
        Tab::PatchCards => render_patch_cards(lang, loaded),
        Tab::AutoBattleData => render_auto_battle_data(lang, loaded),
    }
}

fn render_cover(lang: &LanguageIdentifier) -> Element<'static, Message> {
    container(text(t(lang, "save-cover-description")).size(13))
        .center(Fill)
        .into()
}

// ---------- Folder ----------

#[derive(Default)]
struct GroupedChip {
    count: usize,
    is_regular: bool,
    has_tag1: bool,
    has_tag2: bool,
}

fn render_folder(lang: &LanguageIdentifier, loaded: &Loaded, grouped: bool) -> Element<'static, Message> {
    let Some(chips_view) = loaded.save.view_chips() else {
        return placeholder(t(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let folder_idx = chips_view.equipped_folder_index();
    let regular_idx = chips_view.regular_chip_index(folder_idx);
    let tag_idxs = chips_view.tag_chip_indexes(folder_idx);
    let chips_have_mb = assets.chips_have_mb();

    // Pull the 30-chip folder.
    let mut chips: Vec<Option<tango_dataview::save::Chip>> =
        (0..30).map(|i| chips_view.chip(folder_idx, i)).collect();
    let regular_display_idx = if !assets.regular_chip_is_in_place() {
        if let Some(ri) = regular_idx {
            let c = chips.remove(0);
            chips.insert(ri, c);
            Some(ri)
        } else {
            None
        }
    } else {
        regular_idx
    };

    // Build display items: either grouped (collapsed by chip identity)
    // or per-slot (one row per slot, possibly empty).
    type Item = (Option<tango_dataview::save::Chip>, GroupedChip);
    let items: Vec<Item> = if grouped {
        let mut grouped_map: indexmap::IndexMap<Option<tango_dataview::save::Chip>, GroupedChip> =
            indexmap::IndexMap::new();
        for (i, chip) in chips.iter().enumerate() {
            let g = grouped_map.entry(chip.clone()).or_default();
            g.count += 1;
            if regular_display_idx == Some(i) {
                g.is_regular = true;
            }
            if let Some(t) = tag_idxs {
                if t[0] == i {
                    g.has_tag1 = true;
                }
                if t[1] == i {
                    g.has_tag2 = true;
                }
            }
        }
        grouped_map.into_iter().collect()
    } else {
        chips
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let [t1, t2] = tag_idxs
                    .map(|t| [t[0] == i, t[1] == i])
                    .unwrap_or([false, false]);
                (
                    c,
                    GroupedChip {
                        count: 1,
                        is_regular: regular_display_idx == Some(i),
                        has_tag1: t1,
                        has_tag2: t2,
                    },
                )
            })
            .collect()
    };

    // Header row.
    let mut header_row = row![].spacing(8).align_y(Alignment::Center);
    if grouped {
        header_row = header_row.push(text("#").size(11).width(Length::Fixed(28.0)));
    }
    header_row = header_row
        .push(Space::with_width(Length::Fixed(32.0))) // chip icon column
        .push(text(t(lang, "folder-col-chip")).size(11).width(Length::Fill))
        .push(Space::with_width(Length::Fixed(32.0))) // element icon column
        .push(text(t(lang, "folder-col-power")).size(11).width(Length::Fixed(50.0)));
    if chips_have_mb {
        header_row = header_row.push(text("MB").size(11).width(Length::Fixed(40.0)));
    }
    let header = container(header_row.padding([4, 8])).width(Fill);

    let mut body = column![].spacing(0);
    for (i, (chip, g)) in items.iter().enumerate() {
        let zebra = i % 2 == 0;
        let row_el = chip_row(lang, loaded, chip.as_ref(), g, grouped, chips_have_mb);
        let row_container = container(row_el).width(Fill).style(move |_| container::Style {
            background: if zebra {
                Some(iced::Background::Color(iced::Color::from_rgba8(255, 255, 255, 0.04)))
            } else {
                None
            },
            ..container::Style::default()
        });

        // Hover tooltip with chip image preview + description.
        let chip_id = chip.as_ref().map(|c| c.id);
        let description = chip_id.and_then(|id| loaded.assets.chip(id).and_then(|info| info.description()));
        let image_handle = chip_id.and_then(|id| loaded.chip_images.get(id).cloned().flatten());
        let row_el: Element<'static, Message> = if description.is_some() || image_handle.is_some() {
            let mut tip = column![].spacing(6);
            if let Some((w, h, h_handle)) = image_handle {
                tip = tip.push(
                    Image::new(h_handle)
                        .width(Length::Fixed(w as f32 * 2.0))
                        .height(Length::Fixed(h as f32 * 2.0))
                        .filter_method(iced_image::FilterMethod::Nearest)
                        .content_fit(ContentFit::Contain),
                );
            }
            if let Some(desc) = description {
                tip = tip.push(text(desc).size(12));
            }
            tooltip(
                row_container,
                container(tip).padding(8).style(tooltip_style),
                tooltip::Position::FollowCursor,
            )
            .gap(8)
            .into()
        } else {
            row_container.into()
        };
        body = body.push(row_el);
    }

    // Top chrome row: group-by toggle.
    let chrome = container(
        row![checkbox(t(lang, "folder-group"), grouped).on_toggle(Message::ToggleFolderGrouped)]
            .padding(6),
    )
    .width(Fill);

    column![chrome, horizontal_rule(1), header, horizontal_rule(1), scrollable(body).height(Fill)]
        .width(Fill)
        .height(Fill)
        .into()
}

fn chip_row(
    _lang: &LanguageIdentifier,
    loaded: &Loaded,
    chip: Option<&tango_dataview::save::Chip>,
    g: &GroupedChip,
    grouped: bool,
    chips_have_mb: bool,
) -> Element<'static, Message> {
    let info = chip.and_then(|c| loaded.assets.chip(c.id));
    let chip_class = info.as_ref().map(|i| i.class());
    let dark = info.as_ref().map(|i| i.dark()).unwrap_or(false);

    let bg = row_background(chip_class, dark);

    // Chip icon (cached handle).
    let icon: Element<'static, Message> = match chip.and_then(|c| loaded.chip_icons.get(c.id).cloned().flatten()) {
        Some(h) => Image::new(h)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .filter_method(iced_image::FilterMethod::Nearest)
            .content_fit(ContentFit::Contain)
            .into(),
        None => Space::with_width(Length::Fixed(20.0)).into(),
    };

    // Element icon (cached handle).
    let element_id = info.as_ref().map(|i| i.element());
    let element_icon: Element<'static, Message> = element_id
        .and_then(|id| loaded.element_icons.get(&id).cloned())
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(20.0))
                .height(Length::Fixed(20.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| Space::with_width(Length::Fixed(28.0)).into());

    let name_text = info
        .as_ref()
        .and_then(|i| i.name())
        .unwrap_or_else(|| "???".to_string());
    let code_str = chip.map(|c| c.code.to_string()).unwrap_or_default();
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);

    let mut name_chunk = row![
        text(if chip.is_some() {
            format!("{name_text}  {code_str}")
        } else {
            "—".to_string()
        })
        .size(13)
    ]
    .spacing(6)
    .align_y(Alignment::Center);
    if g.is_regular {
        name_chunk = name_chunk.push(badge("REG", iced::Color::from_rgb8(0xff, 0x42, 0xa5)));
    }
    if g.has_tag1 {
        name_chunk = name_chunk.push(badge("TAG1", iced::Color::from_rgb8(0x29, 0xa1, 0x21)));
    }
    if g.has_tag2 {
        name_chunk = name_chunk.push(badge("TAG2", iced::Color::from_rgb8(0x29, 0xa1, 0x21)));
    }

    let mut r = row![].spacing(8).align_y(Alignment::Center);
    if grouped {
        r = r.push(
            text(format!("{}×", g.count))
                .size(12)
                .width(Length::Fixed(28.0)),
        );
    }
    r = r
        .push(icon)
        .push(container(name_chunk).width(Length::Fill))
        .push(element_icon)
        .push(
            text(if power > 0 {
                format!("{power}")
            } else {
                String::new()
            })
            .size(12)
            .width(Length::Fixed(50.0)),
        );
    if chips_have_mb {
        r = r.push(
            text(if mb > 0 {
                format!("{mb}MB")
            } else {
                String::new()
            })
            .size(12)
            .width(Length::Fixed(40.0)),
        );
    }

    container(r.padding([8, 12]))
        .width(Fill)
        .height(Length::Fixed(40.0))
        .style(move |_| container::Style {
            background: bg.map(iced::Background::Color),
            ..container::Style::default()
        })
        .into()
}

/// Background color for chip rows (mega/giga/dark accents). None = default.
fn row_background(class: Option<tango_dataview::rom::ChipClass>, dark: bool) -> Option<iced::Color> {
    if dark {
        return Some(iced::Color::from_rgb8(0x31, 0x39, 0x5a));
    }
    match class {
        Some(tango_dataview::rom::ChipClass::Mega) => Some(iced::Color::from_rgb8(0x52, 0x84, 0x9c)),
        Some(tango_dataview::rom::ChipClass::Giga) => Some(iced::Color::from_rgb8(0x8c, 0x31, 0x52)),
        _ => None,
    }
}

fn badge(label: &'static str, color: iced::Color) -> Element<'static, Message> {
    container(text(label).size(10).color(iced::Color::WHITE))
        .padding([1, 4])
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(color)),
            border: iced::Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn colored_badge(label: String, bg: iced::Color, text_color: iced::Color) -> Element<'static, Message> {
    container(text(label).size(11).color(text_color))
        .padding([2, 6])
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(bg)),
            border: iced::Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// Solid + plus colors for an NCP color, matching the navicust render.
fn ncp_colors(color: NavicustPartColor) -> (iced::Color, iced::Color) {
    use NavicustPartColor as N;
    match color {
        N::Red => (iced::Color::from_rgb8(0xde, 0x10, 0x00), iced::Color::from_rgb8(0xbd, 0x00, 0x00)),
        N::Pink => (iced::Color::from_rgb8(0xde, 0x8c, 0xc6), iced::Color::from_rgb8(0xbd, 0x6b, 0xa5)),
        N::Yellow => (iced::Color::from_rgb8(0xde, 0xde, 0x00), iced::Color::from_rgb8(0xbd, 0xbd, 0x00)),
        N::Green => (iced::Color::from_rgb8(0x18, 0xc6, 0x00), iced::Color::from_rgb8(0x00, 0xa5, 0x00)),
        N::Blue => (iced::Color::from_rgb8(0x29, 0x84, 0xde), iced::Color::from_rgb8(0x08, 0x60, 0xb8)),
        N::White => (iced::Color::from_rgb8(0xde, 0xde, 0xde), iced::Color::from_rgb8(0xbd, 0xbd, 0xbd)),
        N::Orange => (iced::Color::from_rgb8(0xde, 0x7b, 0x00), iced::Color::from_rgb8(0xbd, 0x5a, 0x00)),
        N::Purple => (iced::Color::from_rgb8(0x94, 0x00, 0xce), iced::Color::from_rgb8(0x73, 0x00, 0xad)),
        N::Gray => (iced::Color::from_rgb8(0x84, 0x84, 0x84), iced::Color::from_rgb8(0x63, 0x63, 0x63)),
    }
}

fn effect_badge(e: &tango_dataview::rom::PatchCard56Effect, enabled: bool) -> Element<'static, Message> {
    let name = e.name.clone().unwrap_or_else(|| "???".to_string());
    let bg = if enabled {
        if e.is_debuff {
            iced::Color::from_rgb8(0xb5, 0x5a, 0xde)
        } else {
            iced::Color::from_rgb8(0xff, 0xbd, 0x18)
        }
    } else {
        iced::Color::from_rgb8(0xbd, 0xbd, 0xbd)
    };
    colored_badge(name, bg, iced::Color::BLACK)
}

fn tooltip_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba8(0, 0, 0, 0.85))),
        text_color: Some(iced::Color::WHITE),
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: iced::Color::from_rgba8(255, 255, 255, 0.2),
        },
        ..Default::default()
    }
}

// ---------- Navi ----------

fn render_navi(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, Message> {
    let Some(navi_view) = loaded.save.view_navi() else {
        return placeholder(t(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    match navi_view {
        tango_dataview::save::NaviView::LinkNavi(v) => {
            let navi_id = v.navi();
            let name = assets
                .navi(navi_id)
                .and_then(|n| n.name())
                .unwrap_or_else(|| format!("Navi #{navi_id}"));
            let emblem: Element<'static, Message> = loaded
                .navi_emblems
                .get(&navi_id)
                .cloned()
                .map(|h| {
                    Image::new(h)
                        .width(Length::Fixed(48.0))
                        .height(Length::Fixed(48.0))
                        .filter_method(iced_image::FilterMethod::Nearest)
                        .content_fit(ContentFit::Contain)
                        .into()
                })
                .unwrap_or_else(|| Space::with_height(Length::Fixed(48.0)).into());
            container(
                column![
                    emblem,
                    text(name).size(22),
                    text(format!("{}: #{navi_id}", t(lang, "navi-id"))).size(12),
                ]
                .spacing(8)
                .padding(16)
                .align_x(Alignment::Center),
            )
            .center(Fill)
            .into()
        }
        tango_dataview::save::NaviView::Navicust(v) => render_navicust(lang, loaded, v.as_ref()),
    }
}

fn render_navicust(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
    v: &dyn tango_dataview::save::NavicustView,
) -> Element<'static, Message> {
    let assets = loaded.assets.as_ref();
    // BN4/5/6 don't have styles — `view.style()` is None there. Only
    // surface the row when the save actually exposes a style id.
    let style_name: Option<String> = v
        .style()
        .map(|id| assets.style(id).and_then(|s| s.name()).unwrap_or_else(|| t(lang, "navi-style-unset")));
    let [cols, rows_n] = v.size();

    // Big rendered grid (tiny-skia, cached at load time). Scale to fit
    // roughly half the pane width while preserving aspect ratio.
    let grid_el: Element<'static, Message> = match loaded.navicust_image.as_ref() {
        Some((w, h, handle)) => {
            // ~60px per square, but we don't want a 480×480 PNG to
            // dominate the pane. Cap to 360px wide, scale h proportionally.
            let cap_w = 360.0_f32;
            let scale = (cap_w / *w as f32).min(1.0);
            let dw = (*w as f32 * scale) as u16;
            let dh = (*h as f32 * scale) as u16;
            Image::new(handle.clone())
                .width(Length::Fixed(dw as f32))
                .height(Length::Fixed(dh as f32))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        }
        None => text(format!(
            "{}: {} × {}",
            t(lang, "navicust-grid-size"),
            cols,
            rows_n
        ))
        .size(12)
        .into(),
    };

    // Parts list: two columns — solid parts (left), plus parts (right) —
    // each colored by NCP color, with hover tooltip showing description.
    let mut solid_col = column![].spacing(4);
    let mut plus_col = column![].spacing(4);
    let mut installed_solid = 0;
    let mut installed_plus = 0;
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else {
            continue;
        };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let part_name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        let description = info.description();
        let is_solid = info.is_solid();
        let (solid_color, plus_color) = info
            .color()
            .map(ncp_colors)
            .unwrap_or((
                iced::Color::from_rgb8(0xbd, 0xbd, 0xbd),
                iced::Color::from_rgb8(0x88, 0x88, 0x88),
            ));
        let bg = if is_solid { solid_color } else { plus_color };
        let badge_el = colored_badge(part_name, bg, iced::Color::BLACK);
        let badge_el: Element<'static, Message> = if let Some(desc) = description {
            tooltip(
                badge_el,
                container(text(desc).size(12)).padding(8).style(tooltip_style),
                tooltip::Position::FollowCursor,
            )
            .gap(8)
            .into()
        } else {
            badge_el
        };
        if is_solid {
            installed_solid += 1;
            solid_col = solid_col.push(badge_el);
        } else {
            installed_plus += 1;
            plus_col = plus_col.push(badge_el);
        }
    }
    if installed_solid + installed_plus == 0 {
        solid_col = solid_col.push(text(t(lang, "navicust-empty")).size(12));
    }
    let parts_list = row![solid_col, plus_col].spacing(12);

    let mut col = column![].spacing(4).padding(16);
    if let Some(name) = style_name {
        col = col.push(text(format!("{}: {}", t(lang, "navi-style"), name)).size(16));
        col = col.push(Space::with_height(8));
    }
    col = col
        .push(grid_el)
        .push(Space::with_height(12))
        .push(text(format!("{}:", t(lang, "navicust-parts"))).size(13))
        .push(parts_list);

    let _ = (cols, rows_n);
    container(scrollable(col)).width(Fill).height(Fill).into()
}

// ---------- Patch cards ----------

fn render_patch_cards(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, Message> {
    let Some(view) = loaded.save.view_patch_cards() else {
        return placeholder(t(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    let mut list = column![].spacing(2).padding(16);
    match view {
        tango_dataview::save::PatchCardsView::PatchCard56s(v) => {
            list = list.push(text(format!("{}: {}", t(lang, "patch-cards-count"), v.count())).size(13));
            list = list.push(horizontal_rule(1));
            for i in 0..v.count() {
                let Some(card) = v.patch_card(i) else { continue };
                let info = assets.patch_card56(card.id);
                let name = info
                    .as_ref()
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| format!("#{}", card.id));
                let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
                let effects: Vec<_> = info.as_ref().map(|c| c.effects()).unwrap_or_default();

                let name_text = if card.enabled {
                    text(name).size(13)
                } else {
                    text(name).size(13).color(iced::Color::from_rgb8(0x70, 0x70, 0x70))
                };
                let name_col = column![
                    name_text,
                    text(format!("{mb}MB")).size(10).color(iced::Color::from_rgb8(0x90, 0x90, 0x90)),
                ]
                .spacing(2);

                let mut ability_col = column![].spacing(2);
                for e in effects.iter().filter(|e| e.is_ability) {
                    ability_col = ability_col.push(effect_badge(e, card.enabled));
                }
                let mut bug_col = column![].spacing(2);
                for e in effects.iter().filter(|e| !e.is_ability) {
                    bug_col = bug_col.push(effect_badge(e, card.enabled));
                }

                let row = row![
                    text(format!("{:>2}", i + 1)).size(12).width(Length::Fixed(24.0)),
                    container(name_col).width(Length::Fill),
                    container(ability_col).width(Length::Fixed(180.0)),
                    container(bug_col).width(Length::Fixed(180.0)),
                ]
                .spacing(8)
                .align_y(Alignment::Start);
                list = list.push(container(row).padding([4, 0]));
            }
        }
        tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
            list = list.push(text(t(lang, "patch-cards-4-title")).size(13));
            list = list.push(horizontal_rule(1));
            for i in 0..6 {
                let card = v.patch_card(i);
                let info = card.as_ref().and_then(|c| assets.patch_card4(c.id));
                let label = match (card.as_ref(), info.as_ref()) {
                    (Some(c), Some(i)) if c.enabled => i.name().unwrap_or_else(|| format!("#{}", c.id)),
                    _ => "—".to_string(),
                };
                let effect = info.as_ref().and_then(|i| i.effect());
                let bug = info.as_ref().and_then(|i| i.bug());

                let mut details_col = column![].spacing(2);
                if let Some(e) = effect {
                    details_col = details_col.push(text(e).size(11).color(iced::Color::from_rgb8(0xff, 0xbd, 0x18)));
                }
                if let Some(b) = bug {
                    details_col = details_col.push(text(b).size(11).color(iced::Color::from_rgb8(0xb5, 0x5a, 0xde)));
                }

                let row = row![
                    text(format!("0{}", ['A', 'B', 'C', 'D', 'E', 'F'][i]))
                        .size(12)
                        .width(Length::Fixed(28.0)),
                    text(label).size(13).width(Length::Fill),
                    details_col,
                ]
                .spacing(8)
                .align_y(Alignment::Start);
                list = list.push(container(row).padding([4, 0]));
            }
        }
    }

    container(scrollable(list)).width(Fill).height(Fill).into()
}

// ---------- Auto Battle Data ----------

fn render_auto_battle_data(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, Message> {
    let Some(view) = loaded.save.view_auto_battle_data() else {
        return placeholder(t(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let mat = view.materialized();

    let chip_name = |id: Option<usize>| -> String {
        match id {
            Some(id) => assets
                .chip(id)
                .and_then(|c| c.name())
                .unwrap_or_else(|| format!("#{id}")),
            None => "—".to_string(),
        }
    };

    let section = |title: String, slots: Vec<String>| -> Element<'static, Message> {
        let mut col = column![text(title).size(13), horizontal_rule(1)].spacing(2);
        for s in slots {
            col = col.push(text(s).size(12));
        }
        col.push(Space::with_height(8)).into()
    };

    let list = column![
        section(
            t(lang, "auto-battle-data-secondary-standard-chips"),
            mat.secondary_standard_chips().iter().map(|s| chip_name(*s)).collect(),
        ),
        section(
            t(lang, "auto-battle-data-standard-chips"),
            mat.standard_chips().iter().map(|s| chip_name(*s)).collect(),
        ),
        section(
            t(lang, "auto-battle-data-mega-chips"),
            mat.mega_chips().iter().map(|s| chip_name(*s)).collect(),
        ),
        section(
            t(lang, "auto-battle-data-giga-chip"),
            vec![chip_name(mat.giga_chip())],
        ),
        section(
            t(lang, "auto-battle-data-combos"),
            mat.combos().iter().map(|s| chip_name(*s)).collect(),
        ),
        section(
            t(lang, "auto-battle-data-program-advance"),
            vec![chip_name(mat.program_advance())],
        ),
    ]
    .spacing(2)
    .padding(16);

    container(scrollable(list)).width(Fill).height(Fill).into()
}

fn placeholder(msg: String) -> Element<'static, Message> {
    container(text(msg).size(13)).center(Fill).into()
}
