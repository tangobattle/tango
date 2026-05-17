//! UI icon glyphs, all drawn from the bundled Noto Emoji font (it
//! covers both the BMP miscellaneous-symbol block — ⚙ ▶ ⏸ ↻ ✕ ✎ ✓ —
//! and the SMP emoji block we use for tab badges). Centralized so a
//! later port to a real icon font only has to touch this file.
//!
//! iced 0.13's cosmic-text won't reliably fall back from the default
//! Noto Sans JP family to Noto Emoji for missing glyphs, so every
//! icon `text(...)` MUST be `.font(FONT)` — otherwise it tofus out.
//!
//! Every icon button still gets a tooltip with the plain-text label
//! for the same i18n key as before, so screen readers / non-English
//! locales / colour-blind users aren't relying on the glyph alone.

use iced::Font;

/// The font every icon glyph must be rendered with.
pub const FONT: Font = Font::with_name("Noto Emoji");

// Top-level navigation tabs.
pub const TAB_PLAY: &str = "🎮";
pub const TAB_REPLAYS: &str = "🎞";
pub const TAB_PATCHES: &str = "🧩";
pub const TAB_SETTINGS: &str = "⚙";

// Save sub-tabs.
pub const SAVE_COVER: &str = "👁";
pub const SAVE_NAVI: &str = "🧑";
pub const SAVE_FOLDER: &str = "🗂";
pub const SAVE_PATCH_CARDS: &str = "🎴";
pub const SAVE_AUTO_BATTLE: &str = "🤖";

// Action / transport buttons.
pub const PLAY: &str = "▶";
pub const PAUSE: &str = "⏸";
pub const CLOSE: &str = "✕";
pub const RESCAN: &str = "↻";
pub const UPDATE: &str = "⬇";
pub const FOLDER: &str = "📁";
pub const NEW: &str = "＋";
pub const RENAME: &str = "✎";
pub const DELETE: &str = "🗑";
pub const DUPLICATE: &str = "⎘";
pub const COPY: &str = "📋";
pub const WATCH: &str = "▶";
pub const EXPORT: &str = "⤓";
pub const CONFIRM: &str = "✓";
pub const CANCEL: &str = "✕";

// ----- widget helpers -----

use iced::widget::{button, container, row, text, tooltip, Text};
use iced::{Alignment, Element, Theme};

/// Build a `text(...)` widget configured to render an icon glyph
/// (forces the icon font + bumps the line height so it sits flush
/// with adjacent label text in a row).
pub fn glyph<'a>(g: &'static str, size: u16) -> Text<'a> {
    text(g).size(size).font(FONT)
}

/// Icon-only button with the plain-text label exposed as a hover
/// tooltip. Keep the original i18n string in `label` — that text is
/// still what shows up in tooltips and for screen-reader-style
/// browsing, only the chrome gets the glyph.
pub fn icon_button<'a, M: Clone + 'a>(
    icon: &'static str,
    label: String,
    msg: M,
    text_size: u16,
    padding: [u16; 2],
) -> Element<'a, M> {
    icon_button_maybe(icon, label, Some(msg), text_size, padding)
}

/// Same as [`icon_button`] but with the on_press wrapped in an Option
/// so callers can render a disabled (greyed-out, no on_press) variant
/// without duplicating the chrome.
pub fn icon_button_maybe<'a, M: Clone + 'a>(
    icon: &'static str,
    label: String,
    msg: Option<M>,
    text_size: u16,
    padding: [u16; 2],
) -> Element<'a, M> {
    let mut btn = button(glyph(icon, text_size)).padding(padding);
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
