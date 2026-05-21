use crate::app::{TEXT_BODY, TEXT_CAPTION, TEXT_DISPLAY};
use crate::i18n::t;
use crate::selection::Loaded;
use iced::widget::{column, container, image as iced_image, row, scrollable, stack, text, tooltip, Image, Space};

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
fn tab_icon(tab: Tab) -> lucide_icons::Icon {
    use lucide_icons::Icon;
    match tab {
        Tab::Cover => Icon::Eye,
        Tab::Navi => Icon::Bot,
        Tab::Folder => Icon::Files,
        Tab::PatchCards => Icon::CreditCard,
        Tab::AutoBattleData => Icon::Swords,
    }
}

/// Persistent UI state for [`view`]. The active tab + folder
/// grouping live here so callers don't have to mirror the fields
/// themselves; apply incoming [`Action`]s via [`State::apply`].
/// The `body_scroll_id` is per-instance unique so multiple
/// save_views on screen at once (e.g. play tab + in-session
/// opponent panel) have distinct scrollable identities.
#[derive(Clone)]
pub struct State {
    pub active_tab: Option<Tab>,
    pub folder_grouped: bool,
    body_scroll_id: iced::widget::Id,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            active_tab: None,
            folder_grouped: true,
            body_scroll_id: iced::widget::Id::unique(),
        }
    }

    /// Apply an `Action` to the state. `CopyTab` is left for the
    /// caller to handle (clipboard side-effects can't happen inside
    /// `apply`); everything else is folded in. Returns a Task the
    /// caller should run — used for save-view-internal side
    /// effects (notably the scroll-to-top snap on a tab change)
    /// so hosts don't have to know about them.
    pub fn apply(&mut self, action: &Action) -> iced::Task<Action> {
        match action {
            Action::SelectTab(t) => {
                self.active_tab = Some(*t);
                iced::widget::operation::snap_to(
                    self.body_scroll_id.clone(),
                    iced::widget::scrollable::RelativeOffset::START,
                )
            }
            Action::ToggleFolderGrouped(g) => {
                self.folder_grouped = *g;
                iced::Task::none()
            }
            Action::CopyTab(_) | Action::CopyTabImage(_) | Action::PlayClicked => iced::Task::none(),
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
    /// Embedder-defined "start single-player here" action.
    /// Emitted by the Play button rendered in the save_view tab
    /// strip when [`view`] is called with `play_button = Some(_)`.
    /// The play tab routes this to `Effect::StartSinglePlayer`;
    /// other embedders (replay, opponent panel) pass `None` and
    /// the button isn't rendered.
    PlayClicked,
}

/// Wholesale save-view widget: tab strip with Lucide icons, optional
/// per-tab extras (folder group toggle, copy buttons), and the body.
/// Embedders just call this and `.map(Message::SaveViewAction)`.
///
/// `play_button`:
///   * `None`        — no Play button in the tab strip.
///   * `Some(true)`  — Play button rendered and enabled.
///   * `Some(false)` — Play button rendered but disabled (e.g.
///     while a netplay lobby is active and singleplayer would
///     conflict with the open session).
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
    streamer_mode: bool,
    play_button: Option<bool>,
) -> Element<'a, Action> {
    use crate::widgets;
    use iced::{Alignment, Fill};

    let available = available_tabs(loaded.save.as_ref(), streamer_mode);
    if available.is_empty() {
        return placeholder(t!(lang, "save-empty"));
    }
    let active = state
        .active_tab
        .filter(|t| available.contains(t))
        .unwrap_or(available[0]);

    let mut tab_row = row![].spacing(2).align_y(Alignment::Center);
    for tab in &available {
        let label = match tab {
            Tab::Cover => t!(lang, "save-tab-cover"),
            Tab::Navi => t!(lang, "save-tab-navi"),
            Tab::Folder => t!(lang, "save-tab-folder"),
            Tab::PatchCards => t!(lang, "save-tab-patch-cards"),
            Tab::AutoBattleData => t!(lang, "save-tab-auto-battle-data"),
        };
        tab_row = tab_row.push(widgets::tab_button(tab_icon(*tab), label, Action::SelectTab(*tab), *tab == active));
    }
    tab_row = tab_row.push(horizontal_space());
    // Tab strip's outer spacing is tight (2 px between tabs) but
    // extras / Play sit visually grouped on the right and want a
    // looser internal rhythm matching the copy-button row's own
    // spacing. Compose them into one tail row.
    let mut tail = row![].spacing(6).align_y(Alignment::Center);
    if let Some(extras) = tab_extras(lang, active, state, loaded) {
        tail = tail.push(extras);
    }
    if let Some(enabled) = play_button {
        use lucide_icons::Icon;
        let label = row![Icon::Play.widget(), text(t!(lang, "play-play"))]
            .spacing(6)
            .align_y(Alignment::Center);
        let mut btn = iced::widget::button(label).padding([4, 10]);
        if enabled {
            btn = btn.style(widgets::primary_button).on_press(Action::PlayClicked);
        } else {
            btn = btn.style(widgets::neutral);
        }
        tail = tail.push(btn);
    }
    tab_row = tab_row.push(tail);

    let opts = RenderOpts {
        folder_grouped: state.folder_grouped,
    };
    let body = render::<Action>(lang, active, loaded, opts);

    let tab_pane = container(tab_row.padding([4, 8]))
        .width(Fill)
        .style(widgets::pane);
    // Body: each render_* returns one-or-more pane-styled
    // containers stacked into an Element. We wrap that whole
    // group in a Shrink-height scrollable so when its panes don't
    // fill the available space the column hugs them, and when
    // they do the user can scroll past the visible window. The
    // per-instance id is what [`State::apply`] snaps to the top
    // on tab changes.
    let body_scrollable = scrollable(body).id(state.body_scroll_id.clone()).width(Fill);
    column![tab_pane, body_scrollable]
        .spacing(widgets::PANE_GAP)
        .width(Fill)
        .into()
}

/// Per-tab extras (folder group-by toggle, copy button) shown on the
/// right of the tab strip. `None` = tab has no extras.
fn tab_extras<'a>(
    lang: &'a LanguageIdentifier,
    tab: Tab,
    state: &'a State,
    loaded: &'a Loaded,
) -> Option<Element<'a, Action>> {
    use crate::widgets;
    use lucide_icons::Icon;
    let copy_btn = |tab: Tab| -> Element<'a, Action> {
        widgets::icon_button(
            Icon::ClipboardCopy,
            t!(lang, "save-copy"),
            Action::CopyTab(tab),
            [4.0, 10.0],
        )
    };
    let copy_img_btn = |tab: Tab| -> Element<'a, Action> {
        widgets::icon_button(
            Icon::ImageDown,
            t!(lang, "save-copy-image"),
            Action::CopyTabImage(tab),
            [4.0, 10.0],
        )
    };
    match tab {
        Tab::Folder => Some(
            row![
                iced::widget::checkbox(state.folder_grouped)
                    .label(t!(lang, "folder-group"))
                    .on_toggle(Action::ToggleFolderGrouped)
                    .size(TEXT_BODY)
                    .text_size(12)
                    .style(crate::widgets::chunky_checkbox),
                copy_btn(Tab::Folder),
            ]
            .spacing(10)
            .align_y(iced::Alignment::Center)
            .into(),
        ),
        Tab::PatchCards => Some(copy_btn(Tab::PatchCards)),
        Tab::AutoBattleData => Some(copy_btn(Tab::AutoBattleData)),
        Tab::Navi => {
            // Copy-as-image only emits anything for Navicust saves
            // (LinkNavi has no grid to render). Hide the button
            // outright on non-navicust navis instead of leaving a
            // dead affordance in the tab strip.
            let has_navicust = matches!(
                loaded.save.view_navi(),
                Some(tango_dataview::save::NaviView::Navicust(_))
            );
            let mut tail = row![].spacing(6).align_y(iced::Alignment::Center);
            if has_navicust {
                tail = tail.push(copy_img_btn(Tab::Navi));
            }
            tail = tail.push(copy_btn(Tab::Navi));
            Some(tail.into())
        }
        _ => None,
    }
}

fn horizontal_space() -> iced::widget::Space {
    iced::widget::space::horizontal()
}

/// Plain-text representation of the active save-view tab, for the
/// clipboard. `None` = tab not exportable in this form.
pub fn tab_as_text(_lang: &LanguageIdentifier, tab: Tab, loaded: &Loaded) -> Option<String> {
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
                    if ti.contains(&i) {
                        out.push_str("\t[TAG]");
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
            section("Secondary standard", mat.secondary_standard_chips());
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
                    let name = assets
                        .navi(id)
                        .and_then(|n| n.name())
                        .unwrap_or_else(|| format!("#{id}"));
                    out.push_str(&format!("{name}\n"));
                }
                tango_dataview::save::NaviView::Navicust(v) => {
                    // Style name first (BN3 only), then two TSV
                    // columns: solid parts on the left, plus parts on
                    // the right, lined up row-by-row to match the
                    // side-by-side layout the UI renders. Shorter
                    // column gets blank cells; the trailing tab keeps
                    // a paste into Google Sheets / Excel parsing as
                    // two columns even when the last solid row has
                    // no plus partner.
                    if let Some(style_id) = v.style() {
                        if let Some(name) = assets.style(style_id).and_then(|s| s.name()) {
                            out.push_str(&name);
                            out.push('\n');
                        }
                    }
                    let mut solid = Vec::new();
                    let mut plus = Vec::new();
                    for i in 0..v.count() {
                        let Some(part) = v.navicust_part(i) else {
                            continue;
                        };
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
    let lang = crate::game::region_to_language(loaded.game.region());
    // Clipboard / export path: render at native (high) resolution.
    Some(crate::navicust::render(
        &materialized,
        &layout,
        v.as_ref(),
        loaded.assets.as_ref(),
        &lang,
        None,
    ))
}

fn render_cover<M: 'static>(lang: &LanguageIdentifier) -> Element<'static, M> {
    container(text(t!(lang, "save-cover-description")).size(TEXT_BODY))
        .width(Fill)
        .padding(crate::widgets::PANE_PADDING)
        .style(crate::widgets::pane)
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
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let folder_idx = chips_view.equipped_folder_index();
    let regular_idx = chips_view.regular_chip_index(folder_idx);
    let tag_idxs = chips_view.tag_chip_indexes(folder_idx);
    let chips_have_mb = assets.chips_have_mb();

    // Pull the 30-chip folder.
    let mut chips: Vec<Option<tango_dataview::save::Chip>> = (0..30).map(|i| chips_view.chip(folder_idx, i)).collect();
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
                let [t1, t2] = tag_idxs.map(|t| [t[0] == i, t[1] == i]).unwrap_or([false, false]);
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
    let mut body = column![].spacing(1).padding(0);
    let total_visible = if grouped {
        items.len()
    } else {
        items.iter().filter(|(c, _)| c.is_some()).count()
    };
    let mut visible_idx = 0usize;
    for (chip, g) in &items {
        if !grouped && chip.is_none() {
            continue;
        }
        let chip_id = chip.as_ref().map(|c| c.id);
        let code = chip.as_ref().map(|c| c.code.to_string());
        let is_first = visible_idx == 0;
        let is_last = visible_idx + 1 == total_visible;
        body = body.push(chip_row(
            loaded,
            chip_id,
            code,
            g,
            grouped,
            chips_have_mb,
            visible_idx,
            is_first,
            is_last,
        ));
        visible_idx += 1;
    }

    let _ = grouped;
    // Rows are flush to the pane edges; the outer scrollable in
    // `view` handles vertical overflow once total content exceeds
    // the available height.
    container(body).width(Fill).style(crate::widgets::pane).into()
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
    row_idx: usize,
    is_first: bool,
    is_last: bool,
) -> Element<'static, M> {
    let info = chip_id.and_then(|id| loaded.assets.chip(id));
    let chip_class = info.as_ref().map(|i| i.class());
    let dark = info.as_ref().map(|i| i.dark()).unwrap_or(false);
    let accent = class_accent(chip_class, dark);
    let is_empty_slot = chip_id.is_none();

    // Chip icon — in-game sprite at 28 px so it reads as a chip
    // graphic rather than a row decoration.
    let icon: Element<'static, M> = match chip_id.and_then(|id| loaded.chip_icons.get(id).cloned().flatten()) {
        Some(h) => Image::new(h)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .filter_method(iced_image::FilterMethod::Nearest)
            .content_fit(ContentFit::Contain)
            .into(),
        None => Space::new().width(Length::Fixed(28.0)).into(),
    };

    // Element icon. Same 14→28 (2× native) scaling as the chip
    // icon so both sprites read at the same size and stay on an
    // integer multiple of their source — anything else makes
    // cosmic-text's resampler eat the pixel grid.
    let element_id = info.as_ref().map(|i| i.element());
    let element_icon: Element<'static, M> = element_id
        .and_then(|id| loaded.element_icons.get(&id).cloned())
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(28.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| Space::new().width(Length::Fixed(28.0)).into());

    let name_text = info
        .as_ref()
        .and_then(|i| i.name())
        .unwrap_or_else(|| "???".to_string());
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);

    // Name only — chip code lives in its own right-aligned
    // column below so every row's letters line up cleanly with
    // the element / power / MB stats.
    let title: Element<'static, M> = if is_empty_slot {
        text("—")
            .size(TEXT_BODY)
            .color(iced::Color::from_rgb8(0x60, 0x60, 0x60))
            .into()
    } else {
        text(name_text).size(TEXT_BODY).into()
    };

    // REG / TAG indicators sit inline with the title so the
    // row stays single-line and the card height stops growing
    // with metadata.
    let mut indicator_row = row![].spacing(4).align_y(Alignment::Center);
    if g.is_regular {
        indicator_row = indicator_row.push(badge("REG", iced::Color::from_rgb8(0xff, 0x42, 0xa5)));
    }
    // Tag chips come in pairs (tag1 + tag2). For the chip list
    // it's the chip-IS-a-tag-chip status the user cares about,
    // not which slot — collapse both flags into a single "TAG".
    for _ in 0..(g.has_tag1 as usize + g.has_tag2 as usize) {
        indicator_row = indicator_row.push(badge("TAG", iced::Color::from_rgb8(0x29, 0xa1, 0x21)));
    }

    // Right-side stats: fixed-width right-aligned columns so the
    // numbers line up vertically across rows. Both inherit the theme's
    // text color — no hard-coded white/yellow that breaks on light.
    // `code.filter().map(...)` consumes the String into the
    // Text widget so the resulting Element is `'static` (a
    // `&String` borrow would tie the Element to this stack
    // frame). The filter drops empty codes so we don't render a
    // blank fixed-width slot.
    let code_text: Option<Element<'static, M>> = code.filter(|s| !s.is_empty()).map(|code| {
        container(text(code).size(TEXT_BODY).font(iced::Font::MONOSPACE))
            .width(Length::Fixed(22.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into()
    });
    let power_text: Element<'static, M> =
        container(text(if power > 0 { format!("{power}") } else { String::new() }).size(TEXT_BODY))
            .width(Length::Fixed(50.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into();
    let mb_text: Element<'static, M> = if chips_have_mb {
        container(text(if mb > 0 { format!("{mb}MB") } else { String::new() }).size(TEXT_CAPTION))
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
        .push(container(row![title, indicator_row].spacing(8).align_y(Alignment::Center)).width(Length::Fill))
        .push(element_icon);
    if let Some(code_text) = code_text {
        r = r.push(code_text);
    }
    r = r
        .push(power_text)
        .push(mb_text);

    let card = card_wrap(r.padding([3, 12]).into(), accent, row_idx, is_first, is_last);
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
        container(tip).padding(8).style(chip_tooltip_style(accent)),
        tooltip::Position::FollowCursor,
    )
    .gap(8)
    .into()
}

/// Tooltip chrome for chip hovers — same shape as
/// [`tooltip_style`] but takes the chip's class accent so
/// mega / giga / dark chips get a background that matches the
/// row's left-edge stripe. Standard chips (accent = None) fall
/// back to the default near-black tooltip.
fn chip_tooltip_style(accent: Option<iced::Color>) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme: &iced::Theme| {
        let bg = accent.unwrap_or_else(|| iced::Color::from_rgba8(0, 0, 0, 0.85));
        container::Style {
            background: Some(iced::Background::Color(bg)),
            text_color: Some(iced::Color::WHITE),
            border: iced::Border {
                radius: 4.0.into(),
                width: 1.0,
                color: iced::Color::from_rgba8(255, 255, 255, 0.2),
            },
            ..Default::default()
        }
    }
}

/// Wraps the inner row content with a 4 px colored stripe on the
/// left for mega/giga/dark chip class accents. The outer container
/// carries the standard zebra-row style so every chip row matches
/// the patch-card / ABD / settings-bindings tables visually; the
/// accent strip sits as a sibling element on the left and paints
/// over the zebra wash where present. Rows without an accent
/// reserve the same 6 px gutter so columns line up across rows.
fn card_wrap<M: 'static>(
    inner: Element<'static, M>,
    accent: Option<iced::Color>,
    row_idx: usize,
    is_first: bool,
    is_last: bool,
) -> Element<'static, M> {
    // Match the pane's `radius: 4.0` on edge rows so the strip's solid
    // accent and the zebra wash don't paint into the pane's rounded
    // corners. The strip only ever touches the left edge, so just the
    // top-left / bottom-left corners need rounding there.
    let r = 4.0_f32;
    let mut strip_radius = iced::border::Radius::new(0.0);
    if is_first {
        strip_radius = strip_radius.top_left(r);
    }
    if is_last {
        strip_radius = strip_radius.bottom_left(r);
    }
    let mut outer_radius = iced::border::Radius::new(0.0);
    if is_first {
        outer_radius = outer_radius.top(r);
    }
    if is_last {
        outer_radius = outer_radius.bottom(r);
    }
    let strip: Element<'static, M> = container(iced::widget::Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fill)
        .style(move |_theme: &iced::Theme| container::Style {
            background: accent.map(iced::Background::Color),
            border: iced::Border {
                radius: strip_radius,
                ..Default::default()
            },
            ..Default::default()
        })
        .into();
    let body: Element<'static, M> = container(inner).width(Fill).into();
    container(row![strip, body].height(Length::Shrink))
        .width(Fill)
        .style(move |theme: &iced::Theme| {
            let mut s = crate::widgets::zebra_row(row_idx)(theme);
            s.border.radius = outer_radius;
            s
        })
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
    // Same dimensions as the NaviCust parts badges so the
    // patch-card effect chips and the NCP parts read as
    // family — chunkier than a chrome chip but smaller than a
    // CTA button.
    colored_badge_sized(label, bg, text_color, TEXT_BODY, [3.0, 8.0])
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
                radius: 6.0.into(),
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
        N::Red => (
            iced::Color::from_rgb8(0xde, 0x10, 0x00),
            iced::Color::from_rgb8(0xbd, 0x00, 0x00),
        ),
        N::Pink => (
            iced::Color::from_rgb8(0xde, 0x8c, 0xc6),
            iced::Color::from_rgb8(0xbd, 0x6b, 0xa5),
        ),
        N::Yellow => (
            iced::Color::from_rgb8(0xde, 0xde, 0x00),
            iced::Color::from_rgb8(0xbd, 0xbd, 0x00),
        ),
        N::Green => (
            iced::Color::from_rgb8(0x18, 0xc6, 0x00),
            iced::Color::from_rgb8(0x00, 0xa5, 0x00),
        ),
        N::Blue => (
            iced::Color::from_rgb8(0x29, 0x84, 0xde),
            iced::Color::from_rgb8(0x08, 0x60, 0xb8),
        ),
        N::White => (
            iced::Color::from_rgb8(0xde, 0xde, 0xde),
            iced::Color::from_rgb8(0xbd, 0xbd, 0xbd),
        ),
        N::Orange => (
            iced::Color::from_rgb8(0xde, 0x7b, 0x00),
            iced::Color::from_rgb8(0xbd, 0x5a, 0x00),
        ),
        N::Purple => (
            iced::Color::from_rgb8(0x94, 0x00, 0xce),
            iced::Color::from_rgb8(0x73, 0x00, 0xad),
        ),
        N::Gray => (
            iced::Color::from_rgb8(0x84, 0x84, 0x84),
            iced::Color::from_rgb8(0x63, 0x63, 0x63),
        ),
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

/// Theme-aware muted text color: mix the palette's text into the
/// background until the contrast drops to "secondary". Works on
/// both light + dark themes — alpha-fading the text on a dark bg
/// turns it into a washed-out near-bg blob; mixing yields a true
/// mid-tone gray instead.
pub fn muted_color(theme: &iced::Theme) -> iced::Color {
    let p = theme.palette();
    fn mix(a: iced::Color, b: iced::Color, t: f32) -> iced::Color {
        iced::Color {
            r: a.r * (1.0 - t) + b.r * t,
            g: a.g * (1.0 - t) + b.g * t,
            b: a.b * (1.0 - t) + b.b * t,
            a: 1.0,
        }
    }
    // Heavy mix breaks contrast on Dark (text tops out at 0.9
    // and bg is ~0.18, so 0.45 lands at ~2.8:1 contrast —
    // basically invisible). 0.25 stays around 4:1 on both
    // themes — visibly secondary but still legible.
    mix(p.text, p.background, 0.25)
}

pub fn muted_text_style(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(muted_color(theme)),
    }
}

/// "OK / success" text color tuned for readability on both Light
/// and Dark themes. The default `extended_palette().success.base`
/// is a dark teal that disappears on a dark background, so we
/// reach for the `strong` variant which iced derives by deviating
/// from base toward higher contrast.
pub fn success_text_style(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().success.strong.color),
    }
}

/// Same idea as [`success_text_style`] for danger — the `strong`
/// variant of palette.danger reads brightly on dark backgrounds
/// where the base color washes out.
pub fn danger_text_style(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().danger.strong.color),
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

fn render_navi<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
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
            container(
                column![
                    emblem,
                    text(name).size(TEXT_DISPLAY)
                ]
                .spacing(8)
                .align_x(Alignment::Center),
            )
            .width(Fill)
            .align_x(Alignment::Center)
            .padding(crate::widgets::PANE_PADDING)
            .style(crate::widgets::pane)
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

            let stacked = stack![image, overlay_col]
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
        let (solid_color, plus_color) = info.color().map(ncp_colors).unwrap_or((
            iced::Color::from_rgb8(0xbd, 0xbd, 0xbd),
            iced::Color::from_rgb8(0x88, 0x88, 0x88),
        ));
        let bg = if is_solid { solid_color } else { plus_color };
        let _ = i; // index no longer needed now that the list-highlight is gone
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
        if is_solid {
            installed_solid += 1;
            solid_col = solid_col.push(badge_el);
        } else {
            installed_plus += 1;
            plus_col = plus_col.push(badge_el);
        }
    }
    // Single pane sized to its contents — no "(none installed)"
    // fallback; an empty navicust shows just the rounded image with
    // pane padding around it. `align_x(Center)` centers narrower rows
    // (style header, parts list) horizontally inside the column's
    // shrink-wrapped width without dragging in any Fill that would
    // stretch the pane across the tab.
    let mut content = column![].spacing(8).align_x(Alignment::Center);
    content = content.push(grid_el);
    if installed_solid + installed_plus > 0 {
        // No Fill anywhere here — Fill on a child propagates up
        // through the column, forcing the whole pane to span the tab.
        content = content.push(row![solid_col, plus_col].spacing(12));
    }

    let _ = (cols, rows_n, installed_solid, installed_plus);
    container(content)
        .padding(crate::widgets::PANE_PADDING)
        .style(crate::widgets::pane)
        .into()
}

// ---------- Patch cards ----------

fn render_patch_cards<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(view) = loaded.save.view_patch_cards() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    let mut list = column![].spacing(3).padding(0);
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
                let effects: Vec<_> = info.as_ref().map(|c| c.effects()).unwrap_or_default();

                let name_text = if card.enabled {
                    text(name).size(TEXT_BODY)
                } else {
                    text(name).size(TEXT_BODY).style(muted_text_style)
                };
                let name_col = column![name_text, text(format!("{mb}MB")).size(10).style(muted_text_style),].spacing(2);

                let mut ability_col = column![].spacing(2);
                for e in effects.iter().filter(|e| e.is_ability) {
                    ability_col = ability_col.push(effect_badge(e, card.enabled));
                }
                let mut bug_col = column![].spacing(2);
                for e in effects.iter().filter(|e| !e.is_ability) {
                    bug_col = bug_col.push(effect_badge(e, card.enabled));
                }

                let row = row![
                    text(format!("{:>2}", i + 1))
                        .size(TEXT_CAPTION)
                        .width(Length::Fixed(24.0)),
                    container(name_col).width(Length::Fill),
                    container(ability_col).width(Length::Fixed(180.0)),
                    container(bug_col).width(Length::Fixed(180.0)),
                ]
                .spacing(8)
                .align_y(Alignment::Start);
                list = list.push(container(row).padding([6, 10]).style(crate::widgets::zebra_row(i)));
            }
        }
        tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
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
                    details_col = details_col.push(
                        text(e)
                            .size(TEXT_CAPTION)
                            .color(iced::Color::from_rgb8(0xff, 0xbd, 0x18)),
                    );
                }
                if let Some(b) = bug {
                    details_col = details_col.push(
                        text(b)
                            .size(TEXT_CAPTION)
                            .color(iced::Color::from_rgb8(0xb5, 0x5a, 0xde)),
                    );
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
                list = list.push(container(row).padding([6, 10]).style(crate::widgets::zebra_row(i)));
            }
        }
    }

    container(list).width(Fill).style(crate::widgets::pane).into()
}

// ---------- Auto Battle Data ----------

fn render_auto_battle_data<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(view) = loaded.save.view_auto_battle_data() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let mat = view.materialized();

    let chips_have_mb = assets.chips_have_mb();

    // ABD slots have no chip code and no REG/TAG indicators, so
    // pass `code=None` and a default-zeroed badge struct. Hover
    // preview comes for free from chip_row. Each section becomes
    // its own pane so the outer scrollable in `view` shows them
    // as distinct demarcated regions.
    let section = |title: String, slots: &[Option<usize>]| -> Element<'static, M> {
        let title_el = container(text(title).size(TEXT_BODY)).padding([8, 12]);
        let mut col = column![title_el, Space::new().height(4)].spacing(1);
        let empty_badges = GroupedChip::default();
        let last_idx = slots.len().saturating_sub(1);
        for (idx, id) in slots.iter().enumerate() {
            // is_first stays false — the title row sits above the chips,
            // so no chip row touches the pane's rounded top corners.
            col = col.push(chip_row(
                loaded,
                *id,
                None,
                &empty_badges,
                false,
                chips_have_mb,
                idx,
                false,
                idx == last_idx,
            ));
        }
        container(col).width(Fill).style(crate::widgets::pane).into()
    };

    column![
        section(
            t!(lang, "auto-battle-data-secondary-standard-chips"),
            mat.secondary_standard_chips(),
        ),
        section(t!(lang, "auto-battle-data-standard-chips"), mat.standard_chips(),),
        section(t!(lang, "auto-battle-data-mega-chips"), mat.mega_chips()),
        section(t!(lang, "auto-battle-data-giga-chip"), &[mat.giga_chip()]),
        section(t!(lang, "auto-battle-data-combos"), mat.combos()),
        section(t!(lang, "auto-battle-data-program-advance"), &[mat.program_advance()],),
    ]
    .spacing(crate::widgets::PANE_GAP)
    .width(Fill)
    .into()
}

fn placeholder<M: 'static>(msg: String) -> Element<'static, M> {
    container(text(msg).size(TEXT_BODY))
        .width(Fill)
        .padding(crate::widgets::PANE_PADDING)
        .style(crate::widgets::pane)
        .into()
}
