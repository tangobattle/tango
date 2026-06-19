//! Tango's iced `Theme` builder. The dark/light palettes are
//! custom — not iced's stock Light/Dark — so they live here
//! rather than picking from `iced::Theme::DARK` etc. Used by
//! `App::theme` (live theme for the window chrome) and by any
//! `view` function that needs to derive a theme-aware style
//! before iced has handed it the live `Theme` (e.g. markdown
//! link color in the About panel).

use crate::config;
use iced::Theme;

/// The accent color used across the app — primary CTA buttons,
/// the active tab chip, panel frames, the cyberworld backdrop,
/// markdown link color in the About panel, etc. Same green the
/// legacy egui app uses, kept in one const so we never
/// accidentally drift to a different shade. (The Legacy
/// Collection restyle briefly ran the PET cyan here; the
/// structure stayed, the color came back home.)
pub const TANGO_GREEN: iced::Color =
    iced::Color::from_rgb(0x4c as f32 / 255.0, 0xaf as f32 / 255.0, 0x50 as f32 / 255.0);

/// The Legacy Collection's selection gold — BNLC paints the picked
/// list row / focused thumbnail in this yellow with dark ink text.
/// Used by `widgets::list_item` for selected rows so "what you've
/// picked" reads in a different register from the green chrome.
pub const SELECT_YELLOW: iced::Color =
    iced::Color::from_rgb(0xff as f32 / 255.0, 0xd2 as f32 / 255.0, 0x3d as f32 / 255.0);

pub fn theme_for(config: &config::Config) -> Theme {
    // Tango palettes — these aren't tweaks of iced's stock Light /
    // Dark anymore. The dark variant keeps the Battle Network
    // Legacy Collection structure (glowing panel frames, gold
    // selection, the drawn cyberworld backdrop) but runs it in
    // tango's own green on the original deep navy — a bluer
    // "PET blue" base was tried twice and clashes with the green
    // chrome. Light is a warm cream + slate set tuned to feel like
    // the same UI under daylight, not a separate identity.
    match config.theme {
        config::ThemeMode::Light => Theme::custom(
            "Tango Light".to_string(),
            iced::theme::Palette {
                background: iced::Color::from_rgb(0xf3 as f32 / 255.0, 0xee as f32 / 255.0, 0xdc as f32 / 255.0),
                text: iced::Color::from_rgb(0x14 as f32 / 255.0, 0x22 as f32 / 255.0, 0x34 as f32 / 255.0),
                primary: TANGO_GREEN,
                success: TANGO_GREEN,
                warning: iced::Color::from_rgb(0xb7 as f32 / 255.0, 0x7e as f32 / 255.0, 0x33 as f32 / 255.0),
                danger: iced::Color::from_rgb(0xd1 as f32 / 255.0, 0x3a as f32 / 255.0, 0x3a as f32 / 255.0),
            },
        ),
        config::ThemeMode::Dark => Theme::custom(
            "Tango Dark".to_string(),
            iced::theme::Palette {
                // Neutral charcoal, the faintest hair cool. The
                // base went navy ("too blue" next to the green
                // chrome), then green-black ("way too green
                // everywhere") — the lesson both times: the base
                // shouldn't carry the accent's hue at all, just sit
                // dark and let the green chrome and gold selection
                // be the color. Still darker than stock iced Dark
                // (0x2b2d31) so the neon green actually glows.
                background: iced::Color::from_rgb(0x0e as f32 / 255.0, 0x10 as f32 / 255.0, 0x11 as f32 / 255.0),
                // Neutral off-white to match — any tinted white
                // (the old cyan, then green) casts its hue onto
                // every surface mixed from it.
                text: iced::Color::from_rgb(0xec as f32 / 255.0, 0xee as f32 / 255.0, 0xed as f32 / 255.0),
                primary: TANGO_GREEN,
                success: TANGO_GREEN,
                warning: iced::Color::from_rgb(0xff as f32 / 255.0, 0xb5 as f32 / 255.0, 0x47 as f32 / 255.0),
                danger: iced::Color::from_rgb(0xff as f32 / 255.0, 0x52 as f32 / 255.0, 0x52 as f32 / 255.0),
            },
        ),
    }
}

pub fn is_gay_time() -> bool {
    use chrono::Datelike;
    chrono::Local::now().month() == 6
}

pub fn rainbow_flag_stops() -> [(f32, iced::Color); 6] {
    [
        (0.0 / 5.0, iced::Color::from_rgb8(0xe4, 0x03, 0x03)), // red
        (1.0 / 5.0, iced::Color::from_rgb8(0xff, 0x8c, 0x00)), // orange
        (2.0 / 5.0, iced::Color::from_rgb8(0xff, 0xed, 0x00)), // yellow
        (3.0 / 5.0, iced::Color::from_rgb8(0x00, 0x80, 0x26)), // green
        (4.0 / 5.0, iced::Color::from_rgb8(0x00, 0x4d, 0xff)), // blue
        (5.0 / 5.0, iced::Color::from_rgb8(0x75, 0x07, 0x87)), // violet
    ]
}

/// The markdown widget's style for the given theme. iced's
/// `markdown::Style::from(theme)` only derives colors — it leaves the body
/// font at `Font::DEFAULT` (the system sans-serif) and code at the system
/// monospace, ignoring the app's bundled Noto faces. Pin them to our fonts
/// so READMEs / the About panel match the rest of the UI.
pub fn markdown_style(theme: &Theme) -> iced::widget::markdown::Style {
    let mut style = iced::widget::markdown::Style::from(theme);
    style.font = crate::style::DEFAULT_FONT;
    style.inline_code_font = crate::style::MONOSPACE_FONT;
    style.code_block_font = crate::style::MONOSPACE_FONT;
    style
}
