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
pub const CONFIRM: &str = "\u{e06c}"; // check
pub const CANCEL: &str = "\u{e1b2}"; // x

// ----- widget helpers -----

use iced::widget::{button, container, row, text, tooltip, Text};
use iced::{Alignment, Element, Theme};

/// Build a `text(...)` widget configured to render an icon glyph
/// (forces the icon font + bumps the line height so it sits flush
/// with adjacent label text in a row).
pub fn glyph<'a>(g: &'static str, size: u16) -> Text<'a> {
    text(g).size(size).font(FONT)
}

/// Icon-only button for low-emphasis toolbar actions (rescan, copy,
/// open-folder, etc.). Renders with `button::secondary` so a row of
/// these doesn't look like a row of "primary" actions all shouting
/// for attention. The plain-text label is exposed as a hover
/// tooltip.
pub fn icon_button<'a, M: Clone + 'a>(
    icon: &'static str,
    label: String,
    msg: M,
    text_size: u16,
    padding: [u16; 2],
) -> Element<'a, M> {
    icon_button_styled(icon, label, Some(msg), text_size, padding, button::secondary)
}

/// `icon_button` with the on_press wrapped in an Option so callers
/// can render a disabled (greyed-out, no on_press) variant without
/// duplicating the chrome.
pub fn icon_button_maybe<'a, M: Clone + 'a>(
    icon: &'static str,
    label: String,
    msg: Option<M>,
    text_size: u16,
    padding: [u16; 2],
) -> Element<'a, M> {
    icon_button_styled(icon, label, msg, text_size, padding, button::secondary)
}

/// Lower-level helper for callers that need to pick the button
/// style explicitly — `button::primary` for the one emphasized
/// action in a row, `button::danger` for destructive ones, etc.
pub fn icon_button_styled<'a, M: Clone + 'a>(
    icon: &'static str,
    label: String,
    msg: Option<M>,
    text_size: u16,
    padding: [u16; 2],
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, M> {
    let mut btn = button(glyph(icon, text_size)).padding(padding).style(style);
    if let Some(m) = msg {
        btn = btn.on_press(m);
    }
    tooltip(
        btn,
        container(text(label).size(11))
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
    text_size: u16,
    padding: [u16; 2],
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
