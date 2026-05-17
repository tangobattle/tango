//! UI icon glyphs, all drawn from the bundled Lucide icon font
//! (https://lucide.dev). Code points sit in the PUA block at U+E000+;
//! the mapping comes from `lucide-static`'s font/info.json. Swap the
//! `FONT` constant + glyph code points to switch to another icon
//! font (Phosphor, Bootstrap Icons, etc.) without touching call sites.
//!
//! iced 0.13's cosmic-text doesn't auto-fall-back from the default
//! family to a PUA-only icon font, so every icon `text(...)` MUST be
//! `.font(FONT)` — otherwise it tofus out. The helpers in this module
//! handle that.
//!
//! Every icon button still gets a tooltip with the plain-text label
//! for the same i18n key as before, so screen readers / non-English
//! locales / colour-blind users aren't relying on the glyph alone.

use iced::Font;

/// The font every icon glyph must be rendered with.
pub const FONT: Font = Font::with_name("lucide");

// Top-level navigation tabs.
pub const TAB_PLAY: &str = "\u{e0de}"; // gamepad
pub const TAB_REPLAYS: &str = "\u{e0d0}"; // film
pub const TAB_PATCHES: &str = "\u{e29c}"; // puzzle
pub const TAB_SETTINGS: &str = "\u{e154}"; // settings

// Save sub-tabs.
pub const SAVE_COVER: &str = "\u{e0ba}"; // eye
pub const SAVE_NAVI: &str = "\u{e1bb}"; // bot
pub const SAVE_FOLDER: &str = "\u{e0cf}"; // files
pub const SAVE_PATCH_CARDS: &str = "\u{e0aa}"; // credit-card
pub const SAVE_AUTO_BATTLE: &str = "\u{e2b4}"; // swords

// Action / transport buttons.
pub const PLAY: &str = "\u{e13c}"; // play
pub const PAUSE: &str = "\u{e12e}"; // pause
pub const CLOSE: &str = "\u{e1b2}"; // x
pub const RESCAN: &str = "\u{e145}"; // refresh-cw
pub const UPDATE: &str = "\u{e0b2}"; // download
pub const FOLDER: &str = "\u{e0d7}"; // folder
pub const NEW: &str = "\u{e13d}"; // plus
pub const RENAME: &str = "\u{e1f9}"; // pencil
pub const DELETE: &str = "\u{e18d}"; // trash
pub const DUPLICATE: &str = "\u{e09e}"; // copy
pub const COPY: &str = "\u{e09e}"; // copy
pub const WATCH: &str = "\u{e13c}"; // play
pub const EXPORT: &str = "\u{e19e}"; // upload
pub const RENDER: &str = "\u{e29b}"; // clapperboard — replay render action
pub const DICE: &str = "\u{e28b}"; // dice-5 — random link-code generator
pub const FIGHT: &str = "\u{e2b4}"; // swords — Fight button (netplay Play)
pub const CONFIRM: &str = "\u{e06c}"; // check
pub const CANCEL: &str = "\u{e1b2}"; // x

// ----- widget helpers -----

use iced::widget::{button, container, row, text, tooltip, Text};
use iced::{Alignment, Element, Theme};

/// Build a `text(...)` widget configured to render an icon glyph
/// (forces the icon font + bumps the line height so it sits flush
/// with adjacent label text in a row).
pub fn glyph<'a>(g: &'static str, size: f32) -> Text<'a> {
    text(g).size(size).font(FONT)
}

/// Icon-only button for low-emphasis toolbar actions (rescan,
/// copy, open-folder, etc.). Uses [`neutral`] — a soft, theme-
/// aware style that doesn't compete with primary CTAs in the
/// same row. The plain-text label is exposed as a hover tooltip.
pub fn icon_button<'a, M: Clone + 'a>(
    icon: &'static str,
    label: String,
    msg: M,
    text_size: f32,
    padding: [f32; 2],
) -> Element<'a, M> {
    icon_button_styled(icon, label, Some(msg), text_size, padding, neutral)
}

/// `icon_button` with the on_press wrapped in an Option so callers
/// can render a disabled (greyed-out, no on_press) variant without
/// duplicating the chrome.
pub fn icon_button_maybe<'a, M: Clone + 'a>(
    icon: &'static str,
    label: String,
    msg: Option<M>,
    text_size: f32,
    padding: [f32; 2],
) -> Element<'a, M> {
    icon_button_styled(icon, label, msg, text_size, padding, neutral)
}

/// List-item button style for selectable rows (patches list,
/// replays list). Selected row uses the bright primary fill
/// (same look the app shipped with originally); inactive rows
/// are transparent with a faint hover tone. Foreground text on
/// the selected row picks up `primary.base.text` (iced contrasts
/// it against the bg) so subtitles must opt into inherit-color
/// rather than render as muted gray on top.
pub fn list_item(selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let p = theme.extended_palette();
        if selected {
            return button::primary(theme, status);
        }
        let base = button::Style {
            background: None,
            text_color: theme.palette().text,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        match status {
            button::Status::Active | button::Status::Pressed => base,
            button::Status::Hovered => button::Style {
                background: Some(iced::Background::Color(p.background.weak.color)),
                ..base
            },
            button::Status::Disabled => base,
        }
    }
}

/// Theme-aware "neutral" button style for low-emphasis toolbar
/// actions. Reads as a clear button at rest (faint background
/// tint + 1 px border in the palette's `background.strong`
/// tone), brightens on hover, dims when disabled. Picks up
/// theme palette derivations so it never collides with the
/// accent color and stays readable on both light + dark.
pub fn neutral(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let base = button::Style {
        background: Some(iced::Background::Color(p.background.weak.color)),
        text_color: theme.palette().text,
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: p.background.strong.color,
        },
        ..Default::default()
    };
    match status {
        button::Status::Active | button::Status::Pressed => base,
        button::Status::Hovered => button::Style {
            background: Some(iced::Background::Color(p.background.strong.color)),
            ..base
        },
        button::Status::Disabled => button::Style {
            text_color: crate::save_view::muted_color(theme),
            ..base
        },
    }
}

/// Lower-level helper for callers that need to pick the button
/// style explicitly — `button::primary` for the one emphasized
/// action in a row, `button::danger` for destructive ones, etc.
pub fn icon_button_styled<'a, M: Clone + 'a>(
    icon: &'static str,
    label: String,
    msg: Option<M>,
    text_size: f32,
    padding: [f32; 2],
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, M> {
    let mut btn = button(glyph(icon, text_size)).padding(padding).style(style);
    if let Some(m) = msg {
        btn = btn.on_press(m);
    }
    tooltip(
        btn,
        container(text(label).size(crate::TEXT_CAPTION))
            .padding(6)
            .style(tooltip_chrome),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

/// Icon-plus-label button. Icon and label use distinct fonts (the icon
/// is Noto Emoji, the label is the app default), so they have to be
/// laid out as a Row rather than concatenated into one `text(...)` —
/// iced text widgets can only carry a single font.
pub fn labeled_icon_button<'a, M: Clone + 'a>(
    icon: &'static str,
    label: String,
    msg: M,
    text_size: f32,
    padding: [f32; 2],
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, M> {
    button(
        row![glyph(icon, text_size), text(label).size(text_size)]
            .spacing(8)
            .align_y(Alignment::Center),
    )
    .padding(padding)
    .style(style)
    .on_press(msg)
    .into()
}

/// Flat compact tab — icon + label, transparent until
/// hovered, underlined with a 2 px colored bar when active.
/// Used by both the top-level nav and the save view's sub-tab
/// strip so they look + feel identical.
///
/// Width is whatever the button's content needs. The underline
/// is rendered as a `Stack` overlay so it spans the button's
/// width without forcing the tab to flex.
pub fn tab_button<'a, M: Clone + 'a>(
    icon: &'static str,
    label: String,
    msg: M,
    active: bool,
) -> Element<'a, M> {
    use iced::widget::{stack, Space};
    let btn = button(
        row![glyph(icon, 13.0), text(label).size(13.0)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .padding([4, 10])
    .style(move |theme: &Theme, status: button::Status| {
        let p = theme.extended_palette();
        let bg = match status {
            button::Status::Hovered if !active => Some(iced::Background::Color(p.background.weak.color)),
            _ => None,
        };
        let text_color = if active {
            p.primary.base.color
        } else {
            theme.palette().text
        };
        button::Style {
            background: bg,
            text_color,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .on_press(msg);

    // Stack picks its size from the FIRST child, so the button
    // drives the tab's width. The underline overlay then takes
    // Fill/Fill bounds inside the stack — which iced clamps to
    // the button's measured bounds, so the bar spans exactly the
    // button width and the tab never grows beyond its content.
    let underline = container(
        container(Space::new().width(iced::Length::Fill))
            .height(iced::Length::Fixed(2.0))
            .width(iced::Length::Fill)
            .style(move |theme: &Theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(if active {
                    theme.palette().primary
                } else {
                    iced::Color::TRANSPARENT
                })),
                ..Default::default()
            }),
    )
    .width(iced::Length::Fill)
    .height(iced::Length::Fill)
    .align_y(iced::alignment::Vertical::Bottom);

    stack![btn, underline].into()
}

fn tooltip_chrome(theme: &Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    iced::widget::container::Style {
        background: Some(iced::Background::Color(p.background.strong.color)),
        text_color: Some(p.background.strong.text),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
