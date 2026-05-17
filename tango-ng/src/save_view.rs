use crate::i18n::t;
use crate::selection::Loaded;
// All `Message` references in this module refer to the Play tab's
// Message enum, because save view widgets are only embedded inside the
// Play tab. The actual rendering helpers (chip rows, navicust, etc.)
// don't construct any messages — they're read-only — so they're
// compatible with any Message type at the iced level, but we type them
// against `play::Message` to keep the call sites simple.
use crate::tabs::play::Message;
use iced::widget::{
    column, container, horizontal_rule, image as iced_image, row, scrollable, text, tooltip, Image, Space,
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
    /// NaviCust part the mouse is hovering over, if any. Used to
    /// highlight the matching badge in the parts list.
    pub hovered_ncp_idx: Option<usize>,
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
        Tab::Navi => render_navi(lang, loaded, opts.hovered_ncp_idx),
        Tab::Folder => render_folder(lang, loaded, opts.folder_grouped),
        Tab::PatchCards => render_patch_cards(lang, loaded),
        Tab::AutoBattleData => render_auto_battle_data(lang, loaded),
    }
}

// `tab_strip_extras` previously lived here and emitted top-level
// Messages; it moved to `tabs::play` so the Play tab can build buttons
// that emit its local `play::Message` directly without going through
// this module.

/// Plain-text representation of the active save-view tab, for the
/// clipboard. `None` = tab not exportable in this form.
pub fn tab_as_text(
    _lang: &LanguageIdentifier,
    tab: Tab,
    loaded: &Loaded,
) -> Option<String> {
    let assets = loaded.assets.as_ref();
    match tab {
        Tab::Folder => {
            let chips_view = loaded.save.view_chips()?;
            let folder_idx = chips_view.equipped_folder_index();
            let regular_idx = chips_view.regular_chip_index(folder_idx);
            let tag_idxs = chips_view.tag_chip_indexes(folder_idx);

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

            let mut out = String::new();
            for (i, chip) in chips.iter().enumerate() {
                let Some(c) = chip else {
                    out.push_str("---\n");
                    continue;
                };
                let name = assets
                    .chip(c.id)
                    .and_then(|info| info.name())
                    .unwrap_or_else(|| "???".to_string());
                out.push_str(&format!("{name}\t{}", c.code));
                if regular_display_idx == Some(i) {
                    out.push_str("\t[REG]");
                }
                if let Some(ti) = tag_idxs {
                    if ti[0] == i {
                        out.push_str("\t[TAG1]");
                    }
                    if ti[1] == i {
                        out.push_str("\t[TAG2]");
                    }
                }
                out.push('\n');
            }
            Some(out)
        }
        Tab::PatchCards => {
            let view = loaded.save.view_patch_cards()?;
            let mut out = String::new();
            match view {
                tango_dataview::save::PatchCardsView::PatchCard56s(v) => {
                    for i in 0..v.count() {
                        let Some(card) = v.patch_card(i) else { continue };
                        let info = assets.patch_card56(card.id);
                        let name = info
                            .as_ref()
                            .and_then(|c| c.name())
                            .unwrap_or_else(|| format!("#{}", card.id));
                        let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
                        out.push_str(&format!(
                            "{name}\t{mb}MB\t{}\n",
                            if card.enabled { "ON" } else { "off" }
                        ));
                    }
                }
                tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
                    for i in 0..6 {
                        let Some(card) = v.patch_card(i) else { continue };
                        let info = assets.patch_card4(card.id);
                        let name = info
                            .as_ref()
                            .and_then(|c| c.name())
                            .unwrap_or_else(|| format!("#{}", card.id));
                        out.push_str(&format!(
                            "0{}\t{name}\t{}\n",
                            ['A', 'B', 'C', 'D', 'E', 'F'][i],
                            if card.enabled { "ON" } else { "off" }
                        ));
                    }
                }
            }
            Some(out)
        }
        Tab::AutoBattleData => {
            let view = loaded.save.view_auto_battle_data()?;
            let mat = view.materialized();
            let chip_name = |id: Option<usize>| match id {
                Some(id) => assets
                    .chip(id)
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| format!("#{id}")),
                None => "—".to_string(),
            };
            let mut out = String::new();
            let mut section = |title: &str, ids: &[Option<usize>]| {
                out.push_str(&format!("[{title}]\n"));
                for id in ids {
                    out.push_str(&chip_name(*id));
                    out.push('\n');
                }
                out.push('\n');
            };
            section(
                "Secondary standard",
                mat.secondary_standard_chips(),
            );
            section("Standard", mat.standard_chips());
            section("Mega", mat.mega_chips());
            section("Giga", &[mat.giga_chip()]);
            section("Combos", mat.combos());
            section("Program advance", &[mat.program_advance()]);
            Some(out)
        }
        Tab::Navi | Tab::Cover => None,
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

    // No column header. The rows themselves carry enough visual info
    // (icon, name+code, element icon, ATK value, MB value) that a label
    // strip would be redundant — and labels are what make it read as a
    // spreadsheet. When ungrouped, skip empty slots so we don't waste
    // a full-height row on each "—".
    let mut body = column![].spacing(4).padding(8);
    for (chip, g) in &items {
        if !grouped && chip.is_none() {
            continue;
        }
        let row_el = chip_row(lang, loaded, chip.as_ref(), g, grouped, chips_have_mb);

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
                row_el,
                container(tip).padding(8).style(tooltip_style),
                tooltip::Position::FollowCursor,
            )
            .gap(8)
            .into()
        } else {
            row_el
        };
        body = body.push(row_el);
    }

    let _ = grouped;
    scrollable(body).height(Fill).width(Fill).into()
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
    let accent = class_accent(chip_class, dark);
    let is_empty_slot = chip.is_none();

    // Chip icon — keep the in-game sprite at native scale (16→28),
    // big enough to recognize without dominating the row.
    let icon: Element<'static, Message> = match chip.and_then(|c| loaded.chip_icons.get(c.id).cloned().flatten()) {
        Some(h) => Image::new(h)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .filter_method(iced_image::FilterMethod::Nearest)
            .content_fit(ContentFit::Contain)
            .into(),
        None => Space::with_width(Length::Fixed(28.0)).into(),
    };

    // Element icon, sits next to the name. Reserve width so columns
    // still align loosely without needing a header.
    let element_id = info.as_ref().map(|i| i.element());
    let element_icon: Element<'static, Message> = element_id
        .and_then(|id| loaded.element_icons.get(&id).cloned())
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(18.0))
                .height(Length::Fixed(18.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| Space::with_width(Length::Fixed(18.0)).into());

    let name_text = info
        .as_ref()
        .and_then(|i| i.name())
        .unwrap_or_else(|| "???".to_string());
    let code_str = chip.map(|c| c.code.to_string()).unwrap_or_default();
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);

    // Name (larger) + code letter as a styled monospace badge.
    let title: Element<'static, Message> = if is_empty_slot {
        text("—")
            .size(14)
            .color(iced::Color::from_rgb8(0x60, 0x60, 0x60))
            .into()
    } else {
        row![
            text(name_text).size(15),
            code_badge(code_str),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    };

    // REG / TAG indicators sit under the title as small badges.
    let mut indicator_row = row![].spacing(4).align_y(Alignment::Center);
    if g.is_regular {
        indicator_row = indicator_row.push(badge("REG", iced::Color::from_rgb8(0xff, 0x42, 0xa5)));
    }
    if g.has_tag1 {
        indicator_row = indicator_row.push(badge("TAG1", iced::Color::from_rgb8(0x29, 0xa1, 0x21)));
    }
    if g.has_tag2 {
        indicator_row = indicator_row.push(badge("TAG2", iced::Color::from_rgb8(0x29, 0xa1, 0x21)));
    }

    // Right-side stats: fixed-width right-aligned columns so the
    // numbers line up vertically across rows. Both inherit the theme's
    // text color — no hard-coded white/yellow that breaks on light.
    let power_text: Element<'static, Message> = container(
        text(if power > 0 { format!("{power}") } else { String::new() }).size(14),
    )
    .width(Length::Fixed(50.0))
    .align_x(iced::alignment::Horizontal::Right)
    .into();
    let mb_text: Element<'static, Message> = if chips_have_mb {
        container(
            text(if mb > 0 { format!("{mb}MB") } else { String::new() }).size(12),
        )
        .width(Length::Fixed(50.0))
        .align_x(iced::alignment::Horizontal::Right)
        .into()
    } else {
        Space::with_width(Length::Fixed(0.0)).into()
    };

    // Count column on the left for grouped mode. Theme-aware text:
    // full strength for count > 1, muted for count == 1 (since "1×" is
    // visual noise) — both readable on light + dark.
    let mut r = row![].spacing(10).align_y(Alignment::Center);
    if grouped {
        let count_is_one = g.count == 1;
        r = r.push(
            text(format!("{}×", g.count))
                .size(14)
                .width(Length::Fixed(28.0))
                .style(move |theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(if count_is_one {
                        muted_color(theme)
                    } else {
                        theme.palette().text
                    }),
                }),
        );
    }
    r = r
        .push(icon)
        .push(
            container(column![title, indicator_row].spacing(2)).width(Length::Fill),
        )
        .push(element_icon)
        .push(power_text)
        .push(mb_text);

    card_wrap(r.padding([8, 12]).into(), accent)
}

/// Wraps the inner row content with a 4 px colored stripe on the left
/// — the outer container's background paints the stripe in the
/// `left: 4` padding band, and the body's opaque page-matched
/// background masks the stripe everywhere else.
///
/// The body has to be opaque (otherwise the accent bleeds through
/// the whole row), but it's set to the theme's page background colour
/// so it visually disappears against the surrounding pane chrome —
/// gives the list a denser look without the shaded-card noise.
fn card_wrap(
    inner: Element<'static, Message>,
    accent: Option<iced::Color>,
) -> Element<'static, Message> {
    let accent_color = accent.unwrap_or(iced::Color::TRANSPARENT);
    let card_body = container(inner)
        .width(Fill)
        .style(|theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(theme.palette().background)),
            ..container::Style::default()
        });

    container(card_body)
        .width(Fill)
        .padding(iced::Padding {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 4.0,
        })
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(accent_color)),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .clip(true)
        .into()
}

/// Folder-style card row for an auto-battle-data slot, which only has
/// a chip id (no code, no REG/TAG indicators).
fn auto_battle_row(
    loaded: &Loaded,
    chip_id: Option<usize>,
    chips_have_mb: bool,
) -> Element<'static, Message> {
    let info = chip_id.and_then(|id| loaded.assets.chip(id));
    let chip_class = info.as_ref().map(|i| i.class());
    let dark = info.as_ref().map(|i| i.dark()).unwrap_or(false);
    let accent = class_accent(chip_class, dark);

    let icon: Element<'static, Message> = match chip_id.and_then(|id| loaded.chip_icons.get(id).cloned().flatten()) {
        Some(h) => Image::new(h)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .filter_method(iced_image::FilterMethod::Nearest)
            .content_fit(ContentFit::Contain)
            .into(),
        None => Space::with_width(Length::Fixed(28.0)).into(),
    };

    let element_id = info.as_ref().map(|i| i.element());
    let element_icon: Element<'static, Message> = element_id
        .and_then(|id| loaded.element_icons.get(&id).cloned())
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(18.0))
                .height(Length::Fixed(18.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| Space::with_width(Length::Fixed(18.0)).into());

    let name_text = info
        .as_ref()
        .and_then(|i| i.name())
        .unwrap_or_else(|| chip_id.map(|id| format!("#{id}")).unwrap_or_default());
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);

    let title: Element<'static, Message> = if chip_id.is_some() {
        text(name_text).size(15).into()
    } else {
        text("—")
            .size(14)
            .color(iced::Color::from_rgb8(0x60, 0x60, 0x60))
            .into()
    };

    let power_text: Element<'static, Message> = container(
        text(if power > 0 { format!("{power}") } else { String::new() }).size(14),
    )
    .width(Length::Fixed(50.0))
    .align_x(iced::alignment::Horizontal::Right)
    .into();
    let mb_text: Element<'static, Message> = if chips_have_mb {
        container(
            text(if mb > 0 { format!("{mb}MB") } else { String::new() }).size(12),
        )
        .width(Length::Fixed(50.0))
        .align_x(iced::alignment::Horizontal::Right)
        .into()
    } else {
        Space::with_width(Length::Fixed(0.0)).into()
    };

    let r = row![
        icon,
        container(title).width(Length::Fill),
        element_icon,
        power_text,
        mb_text,
    ]
    .spacing(10)
    .align_y(Alignment::Center);

    card_wrap(r.padding([8, 12]).into(), accent)
}

/// Accent color for the left edge of a chip row. None = no accent (the
/// row reads as a default chip with no class adornment).
fn class_accent(class: Option<tango_dataview::rom::ChipClass>, dark: bool) -> Option<iced::Color> {
    if dark {
        return Some(iced::Color::from_rgb8(0x4a, 0x55, 0x82));
    }
    match class {
        Some(tango_dataview::rom::ChipClass::Mega) => Some(iced::Color::from_rgb8(0x52, 0x84, 0x9c)),
        Some(tango_dataview::rom::ChipClass::Giga) => Some(iced::Color::from_rgb8(0xc4, 0x52, 0x84)),
        _ => None,
    }
}

fn code_badge(code: String) -> Element<'static, Message> {
    // Theme-aware: opaque "strong" background from the palette, text
    // colour inherits from the theme so it reads on both light + dark.
    container(text(code).size(13).font(iced::Font::MONOSPACE))
        .padding([1, 6])
        .style(|theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(
                theme.extended_palette().background.strong.color,
            )),
            text_color: Some(theme.extended_palette().background.strong.text),
            border: iced::Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
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
    colored_badge_sized(label, bg, text_color, 11, [2, 6])
}

/// Variant that lets callers (NCP parts list) pick a larger text size
/// when the badge is being used as primary content rather than chrome.
fn colored_badge_sized(
    label: String,
    bg: iced::Color,
    text_color: iced::Color,
    size: u16,
    padding: [u16; 2],
) -> Element<'static, Message> {
    colored_badge_highlighted(label, bg, text_color, size, padding, false)
}

/// Badge with an optional "highlighted" treatment (a white outline ring)
/// for showing which NCP is currently under the cursor on the grid.
fn colored_badge_highlighted(
    label: String,
    bg: iced::Color,
    text_color: iced::Color,
    size: u16,
    padding: [u16; 2],
    highlighted: bool,
) -> Element<'static, Message> {
    container(text(label).size(size).color(text_color))
        .padding(padding)
        .style(move |theme: &iced::Theme| {
            let border = if highlighted {
                iced::Border {
                    radius: 3.0.into(),
                    width: 2.0,
                    color: theme.palette().text,
                }
            } else {
                iced::Border {
                    radius: 3.0.into(),
                    ..Default::default()
                }
            };
            container::Style {
                background: Some(iced::Background::Color(bg)),
                border,
                ..Default::default()
            }
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

/// Reduced-opacity text color for "secondary" labels. Works on both
/// light and dark themes because it sits on top of the palette's main
/// text color rather than a hard-coded grey.
pub fn muted_color(theme: &iced::Theme) -> iced::Color {
    let base = theme.palette().text;
    iced::Color { a: 0.55, ..base }
}

pub fn muted_text_style(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(muted_color(theme)),
    }
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

fn render_navi(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
    hovered_ncp_idx: Option<usize>,
) -> Element<'static, Message> {
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
            // Centered portrait — bigger than the in-row icon but still
            // proportional to the pane chrome (was 48 pre-redesign,
            // 120 was way too much).
            let emblem: Element<'static, Message> = loaded
                .navi_emblems
                .get(&navi_id)
                .cloned()
                .map(|h| {
                    Image::new(h)
                        .width(Length::Fixed(64.0))
                        .height(Length::Fixed(64.0))
                        .filter_method(iced_image::FilterMethod::Nearest)
                        .content_fit(ContentFit::Contain)
                        .into()
                })
                .unwrap_or_else(|| Space::with_height(Length::Fixed(64.0)).into());
            container(
                column![
                    emblem,
                    text(name).size(22),
                    text(format!("{}: #{navi_id}", t(lang, "navi-id")))
                        .size(11)
                        .style(muted_text_style),
                ]
                .spacing(8)
                .padding(20)
                .align_x(Alignment::Center),
            )
            .center(Fill)
            .into()
        }
        tango_dataview::save::NaviView::Navicust(v) => render_navicust(lang, loaded, v.as_ref(), hovered_ncp_idx),
    }
}

fn render_navicust(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
    v: &dyn tango_dataview::save::NavicustView,
    hovered_ncp_idx: Option<usize>,
) -> Element<'static, Message> {
    let assets = loaded.assets.as_ref();
    // BN4/5/6 don't have styles — `view.style()` is None there. Only
    // surface the row when the save actually exposes a style id.
    let style_name: Option<String> = v
        .style()
        .map(|id| assets.style(id).and_then(|s| s.name()).unwrap_or_else(|| t(lang, "navi-style-unset")));
    let [cols, rows_n] = v.size();

    // Big rendered grid (tiny-skia, cached at load time). Scale down to
    // ~440 px wide if larger (5×5 grids render around 360 wide native;
    // bigger grids get scaled). Wrapped in mouse_area so hovering over
    // a cell highlights the matching part in the list to the right.
    let grid_el: Element<'static, Message> = match loaded.navicust_render.as_ref() {
        Some(nc) => {
            let cap_w = 440.0_f32;
            let scale = (cap_w / nc.source_w as f32).min(1.0);
            let dw = nc.source_w as f32 * scale;
            let dh = nc.source_h as f32 * scale;
            // Snapshot the lookup data so the on_move closure doesn't
            // borrow `loaded` (the closure has to be 'static).
            let lookup_w = nc.source_w as f32;
            let lookup_h = nc.source_h as f32;
            let body_x = nc.body_origin_x;
            let body_y = nc.body_origin_y;
            let cell_size = nc.cell_size;
            let g_cols = nc.cols;
            let g_rows = nc.rows;
            let cell_idx = nc.cell_part_idx.clone();
            let image = Image::new(nc.handle.clone())
                .width(Length::Fixed(dw))
                .height(Length::Fixed(dh))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain);
            let area = iced::widget::mouse_area(image)
                .on_move(move |p| {
                    let scale_x = lookup_w / dw;
                    let scale_y = lookup_h / dh;
                    let sx = p.x * scale_x - body_x;
                    let sy = p.y * scale_y - body_y;
                    if sx < 0.0 || sy < 0.0 {
                        return Message::NavicustHover(None);
                    }
                    let col = (sx / cell_size) as usize;
                    let row = (sy / cell_size) as usize;
                    if col >= g_cols || row >= g_rows {
                        return Message::NavicustHover(None);
                    }
                    Message::NavicustHover(
                        cell_idx.get(row * g_cols + col).copied().flatten(),
                    )
                })
                .on_exit(Message::NavicustHover(None));
            container(area).center_x(Fill).into()
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
        let hovered = hovered_ncp_idx == Some(i);
        let badge_el = colored_badge_highlighted(part_name, bg, iced::Color::BLACK, 15, [4, 8], hovered);
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

    // Grid pinned to the left at its natural size; parts list takes the
    // remaining width to the right. The parts label sits above the
    // list so it doesn't push the grid down.
    let parts_block = column![
        text(format!("{}:", t(lang, "navicust-parts")))
            .size(13)
            .style(muted_text_style),
        Space::with_height(6),
        parts_list,
    ];

    let layout = row![
        container(grid_el),
        container(parts_block).width(Length::Fill).padding([0, 0]),
    ]
    .spacing(20)
    .align_y(Alignment::Start);

    let mut col = column![].spacing(8).padding(16);
    if let Some(name) = style_name {
        col = col.push(text(format!("{}: {}", t(lang, "navi-style"), name)).size(16));
    }
    col = col.push(layout);

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
                    text(name).size(13).style(muted_text_style)
                };
                let name_col = column![
                    name_text,
                    text(format!("{mb}MB")).size(10).style(muted_text_style),
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

    let chips_have_mb = assets.chips_have_mb();

    let section = |title: String, slots: &[Option<usize>]| -> Element<'static, Message> {
        let mut col = column![text(title).size(13).style(muted_text_style)].spacing(4);
        for id in slots {
            col = col.push(auto_battle_row(loaded, *id, chips_have_mb));
        }
        col.push(Space::with_height(14)).into()
    };

    let list = column![
        section(
            t(lang, "auto-battle-data-secondary-standard-chips"),
            mat.secondary_standard_chips(),
        ),
        section(
            t(lang, "auto-battle-data-standard-chips"),
            mat.standard_chips(),
        ),
        section(t(lang, "auto-battle-data-mega-chips"), mat.mega_chips()),
        section(t(lang, "auto-battle-data-giga-chip"), &[mat.giga_chip()]),
        section(t(lang, "auto-battle-data-combos"), mat.combos()),
        section(
            t(lang, "auto-battle-data-program-advance"),
            &[mat.program_advance()],
        ),
    ]
    .spacing(4)
    .padding(8);

    container(scrollable(list)).width(Fill).height(Fill).into()
}

fn placeholder(msg: String) -> Element<'static, Message> {
    container(text(msg).size(13)).center(Fill).into()
}
