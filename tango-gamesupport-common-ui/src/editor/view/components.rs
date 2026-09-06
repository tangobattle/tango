//! Shared controls and layouts used by the per-game save editors.

use super::Action;
use crate::i18n::t;
use crate::style::{self, TEXT_BODY, TEXT_CAPTION};
use crate::widgets::{muted_color, muted_text_style};
use iced::widget::{button, container, scrollable, text, Space};
use iced::{Alignment, Element, Fill, Length};
use sweeten::widget::{column, pick_list, row, text_input};
use unic_langid::LanguageIdentifier;

/// A sort mode paired with its localized label, for the editors' sort
/// pick_lists — the picker renders options via `Display`, which can't
/// reach the language, so the label is resolved up front. Equality is
/// by mode so the picker can match the current selection.
#[derive(Clone)]
struct SortChoice<S> {
    sort: S,
    label: String,
}

impl<S: PartialEq> PartialEq for SortChoice<S> {
    fn eq(&self, other: &Self) -> bool {
        self.sort == other.sort
    }
}

impl<S> std::fmt::Display for SortChoice<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The filter box + sort picker strip shared by all four editor library
/// panes (folder, navicust palette, patch cards, auto battle data).
/// `search_placeholder` is the filter box's pre-resolved placeholder
/// text (`t!` only takes literal keys, so the lookup stays at the call
/// site); `sort_label` is the sort enum's `label` method.
pub fn library_header<'a, S: Copy + PartialEq + 'static>(
    lang: &LanguageIdentifier,
    search_placeholder: String,
    filter_value: &str,
    on_filter: fn(String) -> Action,
    sorts: &[S],
    current: S,
    sort_label: fn(S, &LanguageIdentifier) -> String,
    on_sort: fn(S) -> Action,
) -> Element<'a, Action> {
    let filter_input = text_input(&search_placeholder, filter_value)
        .on_input(on_filter)
        .padding(style::CONTROL_PADDING)
        .size(TEXT_BODY)
        .width(Fill)
        .style(crate::widgets::chunky_text_input);
    let sort_options: Vec<SortChoice<S>> = sorts
        .iter()
        .map(|&sort| SortChoice {
            sort,
            label: sort_label(sort, lang),
        })
        .collect();
    let sort_selected = sort_options.iter().find(|c| c.sort == current).cloned();
    let sort_pick = pick_list(sort_options, sort_selected, move |c: SortChoice<S>| on_sort(c.sort))
        .padding(style::CONTROL_PADDING)
        .text_size(TEXT_BODY)
        .style(crate::widgets::chunky_pick_list);
    container(
        row![
            filter_input,
            text(t!(lang, "save-edit-sort"))
                .size(TEXT_CAPTION)
                .style(muted_text_style),
            sort_pick,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding(style::HEADER_PADDING)
    .into()
}

/// One editor pane: a header strip pinned above a scrollable body, on
/// the standard pane plate. Every pane in the four editors is this
/// shape.
pub fn editor_pane<'a>(
    header: impl Into<Element<'a, Action>>,
    body: impl Into<Element<'a, Action>>,
) -> Element<'a, Action> {
    container(column![
        header.into(),
        scrollable(body.into())
            .style(crate::widgets::chunky_scrollable)
            .height(Fill)
            .width(Fill)
    ])
    .width(Fill)
    .height(Fill)
    .style(crate::widgets::pane)
    .into()
}

/// The editors' two-pane layout: working set on the left, library /
/// palette on the right.
pub fn editor_panes<'a>(left: Element<'a, Action>, right: Element<'a, Action>) -> Element<'a, Action> {
    row![left, right]
        .spacing(style::PANE_GAP)
        .width(Fill)
        .height(Fill)
        .into()
}

/// The red "Clear" button atop every save-editor pane — identical across the
/// folder / navicust / patch-card / auto-battle-data panes apart from the
/// action it fires.
pub fn clear_all_button<'a>(lang: &LanguageIdentifier, action: Action) -> Element<'a, Action> {
    crate::widgets::labeled_icon_button(
        lucide_icons::Icon::Trash2,
        t!(lang, "save-edit-clear"),
        action,
        style::CONTROL_PADDING,
        crate::widgets::danger_button,
    )
}

/// Standard editor-pane header chrome: a body-size title, any inline stat
/// captions (`extras`), a flexible spacer, then the clear-all button. Shared
/// by the navicust / patch-card / auto-battle-data panes; the folder pane adds
/// a second stats line and builds its own column.
pub fn editor_header<'a>(
    lang: &LanguageIdentifier,
    title: String,
    extras: Vec<Element<'a, Action>>,
    clear_action: Action,
) -> Element<'a, Action> {
    let mut header = row![text(title).size(TEXT_BODY)].spacing(8).align_y(Alignment::Center);
    for extra in extras {
        header = header.push(extra);
    }
    header = header
        .push(Space::new().width(Fill))
        .push(clear_all_button(lang, clear_action));
    container(header).width(Fill).padding(style::HEADER_PADDING).into()
}

/// Caption text that turns danger-red when an editor budget is blown
/// (folder class limits, patch-card MB, folder over-fill) and reads
/// muted otherwise.
pub fn limit_caption<'a>(label: String, over: bool) -> iced::widget::Text<'a> {
    text(label)
        .size(TEXT_CAPTION)
        .style(move |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(if over {
                theme.palette().danger
            } else {
                muted_color(theme)
            }),
        })
}

/// The "✕" button that removes a chip / patch-card from its slot, backing the
/// row out to the library. Identical across the folder and patch-card editors
/// apart from the action it fires.
pub fn remove_button<'a>(action: Action) -> Element<'a, Action> {
    button(lucide_icons::Icon::X.widget().size(TEXT_BODY))
        .padding([3, 8])
        .style(crate::widgets::neutral)
        .on_press(action)
        .into()
}

/// Wrap an editor row's content with the class-accent stripe + zebra
/// background, matching the read-only chip list.
pub fn edit_row_wrap<'a>(
    inner: Element<'a, Action>,
    accent: Option<iced::Color>,
    row_idx: usize,
    leading: Option<Element<'a, Action>>,
) -> Element<'a, Action> {
    edit_row_wrap_maybe_danger_text(inner, accent, row_idx, leading, false)
}

/// [`edit_row_wrap`] with optional danger-red text. The zebra background and
/// chip-class stripe remain unchanged.
pub fn edit_row_wrap_maybe_danger_text<'a>(
    inner: Element<'a, Action>,
    accent: Option<iced::Color>,
    row_idx: usize,
    leading: Option<Element<'a, Action>>,
    danger: bool,
) -> Element<'a, Action> {
    let stripe: Element<'a, Action> = container(Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fill)
        .style(move |_theme: &iced::Theme| container::Style {
            background: accent.map(iced::Background::Color),
            ..Default::default()
        })
        .into();
    // `leading` (e.g. the library's add arrow) sits in the gutter to the
    // left of the accent stripe.
    let mut r = row![].height(Length::Shrink).align_y(Alignment::Center);
    if let Some(lead) = leading {
        r = r.push(container(lead).padding([0, 6]));
    }
    r = r.push(stripe).push(container(inner).width(Fill));
    container(r)
        .width(Fill)
        .style(move |theme: &iced::Theme| {
            let mut style = crate::widgets::zebra_row(row_idx)(theme);
            if danger {
                style.text_color = Some(theme.palette().danger);
            }
            style
        })
        .into()
}

/// A muted grip glyph marking a row as drag-to-reorder. The whole row is the
/// drag surface (sweeten's `Column` owns the gesture); this is just the visual
/// affordance, in a fixed-width cell so rows line up. Wrapped in a `mouse_area`
/// only to show the grab-hand cursor on hover — it sets no handlers, so it
/// doesn't capture the press (the drag gesture still reaches the column).
pub fn drag_handle<'a>() -> Element<'a, Action> {
    use lucide_icons::Icon;
    let grip = container(Icon::GripVertical.widget().size(TEXT_BODY).style(muted_text_style))
        .width(Length::Fixed(16.0))
        .align_x(iced::alignment::Horizontal::Center);
    iced::widget::mouse_area(grip)
        .interaction(iced::mouse::Interaction::Grab)
        .into()
}

/// Drag styling shared by the reorderable folder / patch-card columns. The
/// default sweeten style tints the rows that shift aside with the theme's
/// *primary* color (green in this app); we don't want any overlay, so it's
/// turned off. The floating ghost is softened to a plain panel.
pub fn reorder_drag_style(theme: &iced::Theme) -> sweeten::widget::column::Style {
    let ep = theme.extended_palette();
    let ghost = {
        let mut c = ep.background.weak.color;
        c.a = 0.92;
        c
    };
    sweeten::widget::column::Style {
        scale: 1.02,
        // No tint on the rows that move to open a gap.
        moved_item_overlay: iced::Color::TRANSPARENT,
        ghost_border: iced::Border {
            width: 1.0,
            color: ep.background.strong.color,
            radius: 4.0.into(),
        },
        ghost_background: iced::Background::Color(ghost),
    }
}

/// Small toggle button used for the REG / TAG columns in the folder editor
/// and the patch-card ON column: tinted in `on_color` when active, neutral
/// (greyed) when not. A `None` message renders it disabled (greyed,
/// unclickable) — for the folder REG/TAG toggles when the chip's MB won't
/// fit Regular/Tag memory.
pub fn edit_toggle_maybe<'a>(
    label: &'static str,
    on: bool,
    on_color: iced::Color,
    msg: Option<Action>,
) -> Element<'a, Action> {
    let mut b = button(text(label).size(TEXT_CAPTION)).padding([4, 8]);
    if let Some(msg) = msg {
        b = b.on_press(msg);
    }
    if on {
        b.style(move |theme: &iced::Theme, status| crate::widgets::tinted_button(theme, status, on_color))
            .into()
    } else {
        b.style(crate::widgets::neutral).into()
    }
}

/// Wraps the inner row content with a 4 px colored stripe on the
/// left for mega/giga/dark chip class accents. The outer container
/// carries the standard zebra-row style so every chip row matches
/// the patch-card / ABD / settings-bindings tables visually; the
/// accent strip sits as a sibling element on the left and paints
/// over the zebra wash where present. Rows without an accent
/// reserve the same 6 px gutter so columns line up across rows.
pub fn card_wrap<M: 'static>(
    inner: Element<'static, M>,
    accent: Option<iced::Color>,
    row_idx: usize,
) -> Element<'static, M> {
    card_wrap_maybe_danger_text(inner, accent, row_idx, false)
}

/// [`card_wrap`] with optional danger-red text. The read-only Folder viewer
/// uses this for the same legality feedback as its editable counterpart.
pub fn card_wrap_maybe_danger_text<M: 'static>(
    inner: Element<'static, M>,
    accent: Option<iced::Color>,
    row_idx: usize,
    danger: bool,
) -> Element<'static, M> {
    // Square corners on every row, like `edit_row_wrap` and the library
    // rows: rows sit flush against the pane edges and `zebra_row` is flat
    // by design, so rounding the first/last row only where a list happens
    // to start or end a pane reads as an accidental indent.
    let strip: Element<'static, M> = container(iced::widget::Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fill)
        .style(move |_theme: &iced::Theme| container::Style {
            background: accent.map(iced::Background::Color),
            ..Default::default()
        })
        .into();
    let body: Element<'static, M> = container(inner).width(Fill).into();
    container(row![strip, body].height(Length::Shrink))
        .width(Fill)
        .style(move |theme: &iced::Theme| {
            let mut style = crate::widgets::zebra_row(row_idx)(theme);
            if danger {
                style.text_color = Some(theme.palette().danger);
            }
            style
        })
        .into()
}

pub fn badge<M: 'static>(label: &'static str, color: iced::Color) -> Element<'static, M> {
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

pub fn colored_badge<M: 'static>(label: String, bg: iced::Color, text_color: iced::Color) -> Element<'static, M> {
    // Same dimensions as the NaviCust parts badges so the
    // patch-card effect chips and the NCP parts read as
    // family — chunkier than a chrome chip but smaller than a
    // CTA button.
    colored_badge_sized(label, bg, text_color, TEXT_BODY, [3.0, 8.0], Fill)
}

/// Variant that lets callers (NCP parts list) pick a larger text size
/// when the badge is being used as primary content rather than chrome,
/// and a width. Badges stacked in a column pass `Fill` so they all span
/// it and their edges line up instead of going ragged with the label —
/// which only works if that column has a width of its own to fill, since
/// iced sizes a fluid child against its non-fluid siblings and a column
/// of nothing but fluid badges would collapse to zero.
pub fn colored_badge_sized<M: 'static>(
    label: String,
    bg: iced::Color,
    text_color: iced::Color,
    size: f32,
    padding: [f32; 2],
    width: Length,
) -> Element<'static, M> {
    colored_badge_sized_with_danger(label, bg, text_color, size, padding, width, false)
}

/// A colored content badge with an optional theme-aware danger border. Used
/// for NaviCust pieces whose stored grid materialization is illegal without
/// discarding the piece's own program color.
pub fn colored_badge_sized_with_danger<M: 'static>(
    label: String,
    bg: iced::Color,
    text_color: iced::Color,
    size: f32,
    padding: [f32; 2],
    width: Length,
    danger: bool,
) -> Element<'static, M> {
    container(text(label).size(size).color(text_color))
        .padding(padding)
        .width(width)
        .style(move |theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(bg)),
            border: iced::Border {
                color: if danger {
                    theme.palette().danger
                } else {
                    iced::Color::TRANSPARENT
                },
                width: if danger { 2.0 } else { 0.0 },
                radius: 6.0.into(),
            },
            ..Default::default()
        })
        .into()
}

pub fn tooltip_style(_theme: &iced::Theme) -> container::Style {
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

/// One stat as a tight inline pair: a muted caption label with its
/// value flush beside it in plain body text (no stretched gap). Neither
/// half wraps, so a row of these stays one line.
///
/// This is how every stat beside a name is drawn — the navi strip's HP
/// and MegaBuster levels, a cartridge's own facts — so a game's editor
/// that shows one of its own reaches for this rather than restyling the
/// pair.
pub fn stat<M: 'static>(label: String, value: String) -> Element<'static, M> {
    row![
        text(label)
            .size(TEXT_CAPTION)
            .style(muted_text_style)
            .wrapping(text::Wrapping::None),
        text(value).size(TEXT_BODY).wrapping(text::Wrapping::None),
    ]
    .spacing(5)
    .align_y(Alignment::End)
    .into()
}

pub fn placeholder<M: 'static>(msg: String) -> Element<'static, M> {
    // Centered icon-over-message card rather than a bare line of
    // text in the pane corner — the empty state is a whole-pane
    // situation, so let it own the pane like one.
    container(
        column![
            lucide_icons::Icon::FileQuestion
                .widget()
                .size(36.0)
                .style(muted_text_style),
            text(msg).size(crate::style::TEXT_HEADING).style(muted_text_style),
        ]
        .spacing(8)
        .align_x(Alignment::Center),
    )
    .width(Fill)
    .align_x(Alignment::Center)
    .padding(32)
    .style(crate::widgets::pane)
    .into()
}
