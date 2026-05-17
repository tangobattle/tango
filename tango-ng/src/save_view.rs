use crate::i18n::t;
use crate::selection::Loaded;
use crate::{TEXT_BODY, TEXT_CAPTION, TEXT_DISPLAY, TEXT_HEADING};
use iced::widget::rule::horizontal as horizontal_rule;
use iced::widget::{
    column, container, image as iced_image, row, scrollable, stack, text, tooltip, Image, Space,
};

/// Save view is read-only — every interactive bit (NCP hover, chip
/// hover) is handled by tooltip/canvas widgets that manage their own
/// state internally, so render fns never emit caller-visible messages.
/// The Element is generic over the embedder's Message type.
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

pub fn render<M: 'static>(
    lang: &LanguageIdentifier,
    tab: Tab,
    loaded: &Loaded,
    opts: RenderOpts,
) -> Element<'static, M> {
    match tab {
        Tab::Cover => render_cover(lang),
        Tab::Navi => render_navi(lang, loaded),
        Tab::Folder => render_folder(lang, loaded, opts.folder_grouped),
        Tab::PatchCards => render_patch_cards(lang, loaded),
        Tab::AutoBattleData => render_auto_battle_data(lang, loaded),
    }
}

/// Per-tab Lucide icon glyph used by the tab strip in [`view`].
fn tab_icon(tab: Tab) -> &'static str {
    use crate::icons;
    match tab {
        Tab::Cover => icons::SAVE_COVER,
        Tab::Navi => icons::SAVE_NAVI,
        Tab::Folder => icons::SAVE_FOLDER,
        Tab::PatchCards => icons::SAVE_PATCH_CARDS,
        Tab::AutoBattleData => icons::SAVE_AUTO_BATTLE,
    }
}

/// Persistent UI state for [`view`]. The active tab + folder
/// grouping live here so callers don't have to mirror the fields
/// themselves; apply incoming [`Action`]s via [`State::apply`].
#[derive(Default, Clone)]
pub struct State {
    pub active_tab: Option<Tab>,
    pub folder_grouped: bool,
}

impl State {
    pub fn new() -> Self {
        Self { active_tab: None, folder_grouped: true }
    }

    /// Apply an `Action` to the state. `CopyTab` is left for the
    /// caller to handle (clipboard side-effects can't happen inside
    /// `apply`); everything else is folded in.
    pub fn apply(&mut self, action: &Action) {
        match action {
            Action::SelectTab(t) => self.active_tab = Some(*t),
            Action::ToggleFolderGrouped(g) => self.folder_grouped = *g,
            Action::CopyTab(_) | Action::CopyTabImage(_) => {}
        }
    }
}

/// User-driven changes the embedded save view wants to surface. The
/// caller `.map`s its top-level Message onto this and dispatches:
/// most variants just need `state.apply(&action)`; the Copy
/// variants need the caller's `tab_as_text` / `tab_as_image` plus
/// a clipboard write.
#[derive(Debug, Clone)]
pub enum Action {
    SelectTab(Tab),
    ToggleFolderGrouped(bool),
    CopyTab(Tab),
    CopyTabImage(Tab),
}

/// Wholesale save-view widget: tab strip with Lucide icons, optional
/// per-tab extras (folder group toggle, copy buttons), and the body.
/// Embedders just call this and `.map(Message::SaveViewAction)`.
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
    streamer_mode: bool,
) -> Element<'a, Action> {
    use crate::icons;
    use iced::{Alignment, Fill};

    let available = available_tabs(loaded.save.as_ref(), streamer_mode);
    if available.is_empty() {
        return placeholder(t(lang, "save-empty"));
    }
    let active = state
        .active_tab
        .filter(|t| available.contains(t))
        .unwrap_or(available[0]);

    let mut tab_row = row![].spacing(2).align_y(Alignment::End);
    for tab in &available {
        tab_row = tab_row.push(icons::tab_button(
            tab_icon(*tab),
            t(lang, tab_key(*tab)),
            Action::SelectTab(*tab),
            *tab == active,
        ));
    }
    tab_row = tab_row.push(horizontal_space());
    if let Some(extras) = tab_extras(lang, active, state) {
        tab_row = tab_row.push(extras);
    }

    let opts = RenderOpts { folder_grouped: state.folder_grouped };
    let body = render::<Action>(lang, active, loaded, opts);

    column![
        container(tab_row.padding([4, 8])).width(Fill),
        body,
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

/// Per-tab extras (folder group-by toggle, copy button) shown on the
/// right of the tab strip. `None` = tab has no extras.
fn tab_extras<'a>(
    lang: &'a LanguageIdentifier,
    tab: Tab,
    state: &'a State,
) -> Option<Element<'a, Action>> {
    use crate::icons;
    let copy_btn = |tab: Tab| -> Element<'a, Action> {
        icons::icon_button(icons::COPY, t(lang, "save-copy"), Action::CopyTab(tab), 13.0, [4.0, 10.0])
    };
    let copy_img_btn = |tab: Tab| -> Element<'a, Action> {
        icons::icon_button(
            icons::EXPORT,
            t(lang, "save-copy-image"),
            Action::CopyTabImage(tab),
            13.0,
            [4.0, 10.0],
        )
    };
    match tab {
        Tab::Folder => Some(
            row![
                iced::widget::checkbox(state.folder_grouped).label(t(lang, "folder-group"))
                    .on_toggle(Action::ToggleFolderGrouped)
                    .size(TEXT_BODY)
                    .text_size(12),
                copy_btn(Tab::Folder),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center)
            .into(),
        ),
        Tab::PatchCards => Some(copy_btn(Tab::PatchCards)),
        Tab::AutoBattleData => Some(copy_btn(Tab::AutoBattleData)),
        Tab::Navi => Some(
            row![copy_btn(Tab::Navi), copy_img_btn(Tab::Navi)]
                .spacing(6)
                .align_y(iced::Alignment::Center)
                .into(),
        ),
        _ => None,
    }
}

fn horizontal_space() -> iced::widget::Space {
    iced::widget::space::horizontal()
}

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
        Tab::Navi => {
            let view = loaded.save.view_navi()?;
            let mut out = String::new();
            match view {
                tango_dataview::save::NaviView::LinkNavi(v) => {
                    let id = v.navi();
                    let name = assets.navi(id).and_then(|n| n.name()).unwrap_or_else(|| format!("#{id}"));
                    out.push_str(&format!("{name}\n"));
                }
                tango_dataview::save::NaviView::Navicust(v) => {
                    // Style name first (BN3 only), then a flat
                    // list of solid + plus parts — matches the
                    // legacy `.navi-cust-grid` clipboard export.
                    if let Some(style_id) = v.style() {
                        if let Some(name) = assets.style(style_id).and_then(|s| s.name()) {
                            out.push_str(&name);
                            out.push('\n');
                        }
                    }
                    for i in 0..v.count() {
                        let Some(part) = v.navicust_part(i) else {
                            continue;
                        };
                        let info = assets.navicust_part(part.id);
                        let name = info
                            .as_ref()
                            .and_then(|n| n.name())
                            .unwrap_or_else(|| format!("#{}", part.id));
                        out.push_str(&name);
                        out.push('\n');
                    }
                }
            }
            Some(out)
        }
        Tab::Cover => None,
    }
}

/// Render a save-view tab to an RGBA image, for clipboard
/// "copy as image". Currently only Navi/Navicust supports this
/// (the rendered grid is already an image; we just hand back a
/// fresh copy). Returns `None` for tabs without a meaningful
/// image representation.
pub fn tab_as_image(tab: Tab, loaded: &Loaded) -> Option<image::RgbaImage> {
    let nv = loaded.save.view_navi()?;
    let v = match nv {
        tango_dataview::save::NaviView::Navicust(v) => v,
        _ => return None,
    };
    if !matches!(tab, Tab::Navi) {
        return None;
    }
    let layout = loaded.assets.navicust_layout()?;
    let materialized = v.materialized();
    Some(crate::navicust::render(
        &materialized,
        &layout,
        v.as_ref(),
        loaded.assets.as_ref(),
    ))
}

fn render_cover<M: 'static>(lang: &LanguageIdentifier) -> Element<'static, M> {
    container(text(t(lang, "save-cover-description")).size(TEXT_BODY))
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

fn render_folder<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded, grouped: bool) -> Element<'static, M> {
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
    // Tight stack — rows already have their own padding + accent
    // stripe; extra column spacing here adds dead gaps that read
    // as "spreadsheet" rather than "chip list".
    let mut body = column![].spacing(1).padding(8);
    for (chip, g) in &items {
        if !grouped && chip.is_none() {
            continue;
        }
        let chip_id = chip.as_ref().map(|c| c.id);
        let code = chip.as_ref().map(|c| c.code.to_string());
        body = body.push(chip_row(loaded, chip_id, code, g, grouped, chips_have_mb));
    }

    let _ = grouped;
    scrollable(body).height(Fill).width(Fill).into()
}

// `code = None` skips the code badge (Auto Battle Data slots
// have a chip id but no code). `show_count_cell` toggles the
// leading "N×" column — on for the folder's grouped mode, off
// for ABD.
fn chip_row<M: 'static>(
    loaded: &Loaded,
    chip_id: Option<usize>,
    code: Option<String>,
    g: &GroupedChip,
    show_count_cell: bool,
    chips_have_mb: bool,
) -> Element<'static, M> {
    let info = chip_id.and_then(|id| loaded.assets.chip(id));
    let chip_class = info.as_ref().map(|i| i.class());
    let dark = info.as_ref().map(|i| i.dark()).unwrap_or(false);
    let accent = class_accent(chip_class, dark);
    let is_empty_slot = chip_id.is_none();

    // Chip icon — keep the in-game sprite at native scale (16→28),
    // big enough to recognize without dominating the row.
    let icon: Element<'static, M> = match chip_id.and_then(|id| loaded.chip_icons.get(id).cloned().flatten()) {
        Some(h) => Image::new(h)
            .width(Length::Fixed(22.0))
            .height(Length::Fixed(22.0))
            .filter_method(iced_image::FilterMethod::Nearest)
            .content_fit(ContentFit::Contain)
            .into(),
        None => Space::new().width(Length::Fixed(22.0)).into(),
    };

    // Element icon, sits next to the name. Reserve width so columns
    // still align loosely without needing a header.
    let element_id = info.as_ref().map(|i| i.element());
    let element_icon: Element<'static, M> = element_id
        .and_then(|id| loaded.element_icons.get(&id).cloned())
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(16.0))
                .height(Length::Fixed(16.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| Space::new().width(Length::Fixed(16.0)).into());

    let name_text = info
        .as_ref()
        .and_then(|i| i.name())
        .unwrap_or_else(|| "???".to_string());
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);

    // Name + (optional) code badge.
    let title: Element<'static, M> = if is_empty_slot {
        text("—")
            .size(TEXT_BODY)
            .color(iced::Color::from_rgb8(0x60, 0x60, 0x60))
            .into()
    } else if let Some(code_str) = code.filter(|s| !s.is_empty()) {
        row![text(name_text).size(TEXT_BODY), code_badge(code_str)]
            .spacing(6)
            .align_y(Alignment::Center)
            .into()
    } else {
        text(name_text).size(TEXT_BODY).into()
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
    let power_text: Element<'static, M> = container(
        text(if power > 0 { format!("{power}") } else { String::new() }).size(TEXT_BODY),
    )
    .width(Length::Fixed(50.0))
    .align_x(iced::alignment::Horizontal::Right)
    .into();
    let mb_text: Element<'static, M> = if chips_have_mb {
        container(
            text(if mb > 0 { format!("{mb}MB") } else { String::new() }).size(TEXT_CAPTION),
        )
        .width(Length::Fixed(50.0))
        .align_x(iced::alignment::Horizontal::Right)
        .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    // Count column on the left for grouped mode. Theme-aware text:
    // full strength for count > 1, muted for count == 1 (since "1×" is
    // visual noise) — both readable on light + dark.
    let mut r = row![].spacing(10).align_y(Alignment::Center);
    if show_count_cell {
        let count_is_one = g.count == 1;
        r = r.push(
            text(format!("{}×", g.count))
                .size(TEXT_BODY)
                .width(Length::Fixed(22.0))
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

    let card = card_wrap(r.padding([3, 12]).into(), accent);
    // Hover tooltip with chip image preview + description.
    // Always rendered when the chip has either; the folder list
    // and Auto Battle Data both want this affordance, so it
    // lives at the bottom of chip_row instead of as a wrapper
    // the callers have to remember to use.
    let Some(id) = chip_id else {
        return card;
    };
    let description = loaded.assets.chip(id).and_then(|info| info.description());
    let image_handle = loaded.chip_images.get(id).cloned().flatten();
    if description.is_none() && image_handle.is_none() {
        return card;
    }
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
        tip = tip.push(text(desc).size(TEXT_CAPTION));
    }
    tooltip(
        card,
        container(tip).padding(8).style(tooltip_style),
        tooltip::Position::FollowCursor,
    )
    .gap(8)
    .into()
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
fn card_wrap<M: 'static>(
    inner: Element<'static, M>,
    accent: Option<iced::Color>,
) -> Element<'static, M> {
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
            // Square corners — the rounded stripe was reading as
            // a "tab" or "pill" rather than a flush accent edge.
            border: iced::Border::default(),
            ..Default::default()
        })
        .clip(true)
        .into()
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

fn code_badge<M: 'static>(code: String) -> Element<'static, M> {
    // Theme-aware: opaque "strong" background from the palette, text
    // colour inherits from the theme so it reads on both light + dark.
    container(text(code).size(TEXT_BODY).font(iced::Font::MONOSPACE))
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


fn badge<M: 'static>(label: &'static str, color: iced::Color) -> Element<'static, M> {
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

fn colored_badge<M: 'static>(label: String, bg: iced::Color, text_color: iced::Color) -> Element<'static, M> {
    colored_badge_sized(label, bg, text_color, 11.0, [2.0, 6.0])
}

/// Variant that lets callers (NCP parts list) pick a larger text size
/// when the badge is being used as primary content rather than chrome.
fn colored_badge_sized<M: 'static>(
    label: String,
    bg: iced::Color,
    text_color: iced::Color,
    size: f32,
    padding: [f32; 2],
) -> Element<'static, M> {
    container(text(label).size(size).color(text_color))
        .padding(padding)
        .style(move |_theme: &iced::Theme| container::Style {
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

fn effect_badge<M: 'static>(e: &tango_dataview::rom::PatchCard56Effect, enabled: bool) -> Element<'static, M> {
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

fn render_navi<M: 'static>(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
) -> Element<'static, M> {
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
            let emblem: Element<'static, M> = loaded
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
                .unwrap_or_else(|| Space::new().height(Length::Fixed(64.0)).into());
            // Top-aligned column — `.center(Fill)` collapsed to zero
            // inside the replays scrollable (Fill inside infinite-
            // height scroll content evaluates to Shrink), and the
            // user saw an empty pane.
            container(
                column![
                    emblem,
                    text(name).size(TEXT_DISPLAY),
                    text(format!("{}: #{navi_id}", t(lang, "navi-id")))
                        .size(TEXT_CAPTION)
                        .style(muted_text_style),
                ]
                .spacing(8)
                .padding(20)
                .align_x(Alignment::Center),
            )
            .width(Fill)
            .align_x(Alignment::Center)
            .into()
        }
        tango_dataview::save::NaviView::Navicust(v) => render_navicust(lang, loaded, v.as_ref()),
    }
}

fn render_navicust<M: 'static>(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
    v: &dyn tango_dataview::save::NavicustView,
) -> Element<'static, M> {
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
    // Per-cell tooltip overlay: render the image as one layer of a
    // Stack and a column-of-rows-of-cell-sized empty widgets as the
    // second layer. Each cell that's covered by an installed part gets
    // its own tooltip wrapper, so iced's tooltip widget manages the
    // hover state internally — no NavicustHover message round-trip
    // needed.
    let grid_el: Element<'static, M> = match loaded.navicust_render.as_ref() {
        Some(nc) => {
            let cap_w = 440.0_f32;
            let scale = (cap_w / nc.source_w as f32).min(1.0);
            let dw = nc.source_w as f32 * scale;
            let dh = nc.source_h as f32 * scale;
            let body_x = nc.body_origin_x * scale;
            let body_y = nc.body_origin_y * scale;
            let cell_size = nc.cell_size * scale;
            let g_cols = nc.cols;
            let g_rows = nc.rows;

            let image: Element<'static, M> = Image::new(nc.handle.clone())
                .width(Length::Fixed(dw))
                .height(Length::Fixed(dh))
                .filter_method(iced_image::FilterMethod::Nearest)
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
                        let space = Space::new().width(Length::Fixed(cell_size)).height(Length::Fixed(cell_size));
                        tooltip(space, tip, tooltip::Position::FollowCursor).gap(12).into()
                    } else {
                        Space::new().width(Length::Fixed(cell_size)).height(Length::Fixed(cell_size)).into()
                    };
                    cell_row = cell_row.push(cell);
                }
                overlay_col = overlay_col.push(cell_row);
            }

            let stacked = stack![image, overlay_col]
                .width(Length::Fixed(dw))
                .height(Length::Fixed(dh));
            container(stacked).center_x(Fill).into()
        }
        None => text(format!(
            "{}: {} × {}",
            t(lang, "navicust-grid-size"),
            cols,
            rows_n
        ))
        .size(TEXT_CAPTION)
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
        let _ = i; // index no longer needed now that the list-highlight is gone
        let badge_el = colored_badge_sized(part_name, bg, iced::Color::BLACK, 15.0, [4.0, 8.0]);
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
        if is_solid {
            installed_solid += 1;
            solid_col = solid_col.push(badge_el);
        } else {
            installed_plus += 1;
            plus_col = plus_col.push(badge_el);
        }
    }
    if installed_solid + installed_plus == 0 {
        solid_col = solid_col.push(text(t(lang, "navicust-empty")).size(TEXT_CAPTION));
    }
    let parts_list = row![solid_col, plus_col].spacing(12);

    // Grid pinned to the left at its natural size; parts list takes the
    // remaining width to the right. The parts label sits above the
    // list so it doesn't push the grid down.
    let parts_block = column![
        text(format!("{}:", t(lang, "navicust-parts")))
            .size(TEXT_BODY)
            .style(muted_text_style),
        Space::new().height(6),
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
        col = col.push(text(format!("{}: {}", t(lang, "navi-style"), name)).size(TEXT_HEADING));
    }
    col = col.push(layout);

    let _ = (cols, rows_n);
    container(scrollable(col)).width(Fill).height(Fill).into()
}

// ---------- Patch cards ----------

fn render_patch_cards<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(view) = loaded.save.view_patch_cards() else {
        return placeholder(t(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    let mut list = column![].spacing(2).padding(16);
    match view {
        tango_dataview::save::PatchCardsView::PatchCard56s(v) => {
            list = list.push(text(format!("{}: {}", t(lang, "patch-cards-count"), v.count())).size(TEXT_BODY));
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
                    text(name).size(TEXT_BODY)
                } else {
                    text(name).size(TEXT_BODY).style(muted_text_style)
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
                    text(format!("{:>2}", i + 1)).size(TEXT_CAPTION).width(Length::Fixed(24.0)),
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
            list = list.push(text(t(lang, "patch-cards-4-title")).size(TEXT_BODY));
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
                    details_col = details_col.push(text(e).size(TEXT_CAPTION).color(iced::Color::from_rgb8(0xff, 0xbd, 0x18)));
                }
                if let Some(b) = bug {
                    details_col = details_col.push(text(b).size(TEXT_CAPTION).color(iced::Color::from_rgb8(0xb5, 0x5a, 0xde)));
                }

                let row = row![
                    text(format!("0{}", ['A', 'B', 'C', 'D', 'E', 'F'][i]))
                        .size(TEXT_CAPTION)
                        .width(Length::Fixed(22.0)),
                    text(label).size(TEXT_BODY).width(Length::Fill),
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

fn render_auto_battle_data<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(view) = loaded.save.view_auto_battle_data() else {
        return placeholder(t(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let mat = view.materialized();

    let chips_have_mb = assets.chips_have_mb();

    // ABD slots have no chip code and no REG/TAG indicators, so
    // pass `code=None` and a default-zeroed badge struct. Hover
    // preview comes for free from chip_row.
    let section = |title: String, slots: &[Option<usize>]| -> Element<'static, M> {
        let mut col = column![text(title).size(TEXT_BODY).style(muted_text_style)].spacing(1);
        let empty_badges = GroupedChip::default();
        for id in slots {
            col = col.push(chip_row(loaded, *id, None, &empty_badges, false, chips_have_mb));
        }
        col.push(Space::new().height(14)).into()
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

fn placeholder<M: 'static>(msg: String) -> Element<'static, M> {
    container(text(msg).size(TEXT_BODY)).center(Fill).into()
}
