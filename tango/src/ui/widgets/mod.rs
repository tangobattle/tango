//! The app's own widgets, over the shared toolkit in
//! [`tango_ui::widgets`] — re-exported here, so `crate::ui::widgets::*`
//! covers both without call sites caring which side of the gamesupport
//! boundary a widget lives on. The HUD chrome (`hud_bar`,
//! `hud_scanline_top`, `cyber_backdrop`, `panel`), the nav tabs, the
//! ⋮ [`MenuButton`], and the match-analysis chart are all app-only.

pub use tango_ui::widgets::*;

use crate::ui::style::{PANE_GAP, TEXT_BODY, TEXT_CAPTION};
use iced::widget::{button, container, text, tooltip};
use iced::{Alignment, Element, Length, Theme};
use lucide_icons::Icon;
use sweeten::widget::{column, row};

mod chrome;
pub use chrome::*;

mod match_graph;
pub use match_graph::*;

mod styles;
pub use styles::*;

mod menu_button;
pub use menu_button::{MenuButton, MenuItem};

/// A ⋮ "more actions" button: [`icon_button`] chrome on the trigger,
/// the standard dropdown overlay for the actions. `label` is the
/// hover tooltip; each item's message fires on selection (the
/// dropdown closes itself, on selection or click-away). Disabled
/// (greyed, won't open) when `enabled` is false — for rows whose
/// actions all need a selection to act on.
pub fn menu_button<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    items: Vec<MenuItem<M>>,
    enabled: bool,
    padding: [f32; 2],
) -> Element<'a, M> {
    let btn = menu_button::MenuButton::new(
        icon.widget(),
        items,
        enabled,
        padding,
        crate::ui::style::STANDARD_PADDING,
        neutral,
    );
    // Tooltip above, not below — below is where the dropdown lands,
    // and the bubble lingers while the cursor rests on the trigger.
    tooltip(btn, tooltip_bubble(label), tooltip::Position::Top)
        .gap(4)
        .into()
}

/// The replay renderer's shared quality/scale picker. Full replay
/// exports and marked clips both edit the same setting, so they use
/// this one control too: `0` is raw output at native resolution and
/// `1..=10` is a lossy integer upscale.
pub fn replay_export_scale_picker<'a, M: Clone + 'a>(
    lang: &'a unic_langid::LanguageIdentifier,
    scale: u8,
    on_select: impl Fn(u8) -> M,
    on_toggle: Option<fn(bool) -> M>,
) -> Element<'a, M> {
    let scale = scale.min(10);
    let value_label = |scale: u8| {
        if scale == 0 {
            crate::i18n::t!(lang, "replays-export-scale-raw").to_string()
        } else {
            format!("{scale}×")
        }
    };
    let current_label = value_label(scale);
    let items = (0..=10)
        .map(|candidate| MenuItem::toggle(value_label(candidate), on_select(candidate), candidate == scale))
        .collect();
    let mut picker = MenuButton::new(
        row![
            Icon::Scaling.widget().size(14.0),
            text(format!(
                "{}: {}",
                crate::i18n::t!(lang, "replays-export-scale"),
                current_label
            ))
            .size(TEXT_CAPTION),
            Icon::ChevronDown.widget().size(12.0),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        items,
        true,
        [4.0, 8.0],
        crate::ui::style::STANDARD_PADDING,
        neutral,
    )
    .menu_width(144.0);
    if let Some(on_toggle) = on_toggle {
        picker = picker.on_toggle(on_toggle);
    }
    tooltip(
        picker,
        tooltip_bubble(format!(
            "{}: {}",
            crate::i18n::t!(lang, "replays-export-scale"),
            current_label
        )),
        tooltip::Position::Top,
    )
    .gap(4)
    .into()
}

/// A pick_list option: a value paired with a pre-resolved display
/// label. The picker renders options via `Display`, which can't reach
/// the language or any other formatting context, so labels are built
/// when the option list is constructed. Equality is by value only, so
/// selection-matching survives label differences (e.g. a favorites
/// star prefix).
#[derive(Clone, Debug)]
pub struct Choice<T> {
    pub value: T,
    pub label: String,
}

impl<T> Choice<T> {
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }
}

impl<T: PartialEq> PartialEq for Choice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// A caption label stacked over a control — the "form row" shape
/// used by the welcome screen (settings rows use [`option_row`]).
pub fn labeled<'a, M: Clone + 'a>(label: String, ctrl: impl Into<Element<'a, M>>) -> Element<'a, M> {
    sweeten::widget::column![text(label).size(TEXT_CAPTION).style(muted_text_style), ctrl.into(),]
        .spacing(4)
        .into()
}

/// Fixed height of every [`option_row`] — one slot size whatever
/// the control (a text input, a picker, a bare checkbox), so a
/// settings pane reads as an even options list, not a form whose
/// rows breathe with their contents.
const OPTION_ROW_HEIGHT: f32 = 40.0;

/// A full-width "options screen" row: label on the left, control
/// hugging the right edge, every row exactly
/// [`OPTION_ROW_HEIGHT`] tall — the console-menu shape, not a
/// desktop form's caption-over-control. The label is body-sized
/// ink (not a muted caption): on an options screen the setting's
/// name IS the row, not an annotation on it.
pub fn option_row<'a, M: 'a>(label: String, ctrl: impl Into<Element<'a, M>>) -> Element<'a, M> {
    row![
        text(label).size(TEXT_BODY),
        iced::widget::space::horizontal(),
        ctrl.into(),
    ]
    .spacing(12)
    .padding([0, 10])
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .height(Length::Fixed(OPTION_ROW_HEIGHT))
    .into()
}

/// Larger pill for the global top nav (Play / Replays).
/// TEXT_HEADING-sized icon + label so the chrome reads as the
/// primary navigation for the whole app.
pub fn nav_tab_button<'a, M: Clone + 'a>(icon: Icon, label: String, msg: M, active: bool) -> Element<'a, M> {
    pill_tab(icon, Some(label), msg, active, true)
}

/// [`nav_tab_button`] with an attention dot: a small primary-glow pip
/// floated over the pill's top-right corner, for "something is live
/// on this tab while you're looking at another" (e.g. an open lobby).
/// The pip is an overlay, not row content — it takes no layout space,
/// so the pill is exactly [`nav_tab_button`]-sized whether the dot is
/// lit, unlit, or never there, and the tab strip never shifts.
pub fn nav_tab_button_badged<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    msg: M,
    active: bool,
    badge: bool,
) -> Element<'a, M> {
    let pill = pill_tab(icon, Some(label), msg, active, true);
    if !badge {
        return pill;
    }
    pill_tab_badge(pill, |theme| theme.palette().primary)
}

/// Icon-only variant of [`nav_tab_button`] for the right-aligned
/// utility tabs (Patches, Settings).
pub fn nav_icon_tab_button<'a, M: Clone + 'a>(
    icon: Icon,
    tooltip_label: String,
    msg: M,
    active: bool,
) -> Element<'a, M> {
    let stacked = pill_tab(icon, None, msg, active, true);
    tooltip(stacked, tooltip_bubble(tooltip_label), tooltip::Position::Bottom)
        .gap(4)
        .into()
}

/// The standard tab body: a full-width `top` strip above a left/right split,
/// with every gap and the outer inset set to [`PANE_GAP`]. Shared by the
/// Patches and Replays tabs.
pub fn top_split_pane<'a, M: 'a>(
    top: impl Into<Element<'a, M>>,
    left: impl Into<Element<'a, M>>,
    right: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let top: Element<'a, M> = top.into();
    let left: Element<'a, M> = left.into();
    let right: Element<'a, M> = right.into();
    column![top, row![left, right].spacing(PANE_GAP).height(Length::Fill)]
        .spacing(PANE_GAP)
        .padding(PANE_GAP)
        .height(Length::Fill)
        .into()
}

/// A detail pane's empty state: `message` centered on the [`pane`] plate.
/// Shown by the Patches / Replays tabs when nothing is selected.
pub fn pane_prompt<'a, M: 'a>(message: String) -> Element<'a, M> {
    container(text(message).size(TEXT_BODY))
        .center(Length::Fill)
        .style(pane)
        .into()
}

/// The standard dropdown: sweeten's `pick_list` with the
/// [`chunky_pick_list`] chrome and [`STANDARD_PADDING`] applied.
/// Callers chain extras (`.placeholder`, `.width`, `.disabled`) on
/// the returned picker; compact in-pane variants (CONTROL_PADDING +
/// smaller text) keep hand-building.
///
/// [`STANDARD_PADDING`]: crate::ui::style::STANDARD_PADDING
pub fn picker<'a, T, L, V, M>(
    options: L,
    selected: Option<V>,
    on_selected: impl Fn(T) -> M + 'a,
) -> sweeten::widget::PickList<'a, T, L, V, M>
where
    T: ToString + PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
    M: Clone,
{
    sweeten::widget::pick_list(options, selected, on_selected)
        .padding(crate::ui::style::STANDARD_PADDING)
        .style(chunky_pick_list)
}

/// One patch download, as the same row wherever it shows up: the
/// patches tab, the play strip, the lobby band and the replay detail
/// all fetch the same packages, and used to each say so differently.
///
/// Deliberately small — a short bar and a caption on one line, not a
/// full-width meter. Running draws a determinate bar (flat until the
/// server tells us the size); failed drops the bar for the caption in
/// danger colour. `retry` and `cancel` are the surface's own messages;
/// pass `None` to leave the affordance out. Captions arrive
/// pre-resolved: this decides layout, not wording.
pub fn download_row<'a, M: Clone + 'a>(
    caption: String,
    fraction: Option<f32>,
    failed: bool,
    retry: Option<(String, M)>,
    cancel: Option<(String, M)>,
) -> Element<'a, M> {
    let mut controls = row![].spacing(6).align_y(Alignment::Center);
    if !failed {
        controls = controls.push(
            iced::widget::progress_bar(0.0..=1.0, fraction.unwrap_or(0.0))
                .girth(Length::Fixed(3.0))
                .length(Length::Fixed(56.0))
                .style(slim_progress_bar),
        );
    }
    let style: fn(&Theme) -> iced::widget::text::Style = if failed { danger_text_style } else { muted_text_style };
    controls = controls.push(text(caption).size(TEXT_CAPTION).style(style));
    for (icon, action) in [(Icon::RefreshCw, retry), (Icon::X, cancel)] {
        if let Some((label, msg)) = action {
            controls = controls.push(icon_button(icon, label, msg, [1.0, 1.0]));
        }
    }
    controls.into()
}
