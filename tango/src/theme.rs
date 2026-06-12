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
/// the active tab chip, panel frames, markdown link color in the
/// About panel, etc. The Legacy Collection's PET cyan: the glowing
/// border color of every panel in BNLC's menus. Kept in one const
/// so we never accidentally drift to a different shade. (Tango's
/// old egui green is fully retired — the LC restyle runs cyan
/// through and through.)
pub const TANGO_CYAN: iced::Color =
    iced::Color::from_rgb(0x4e as f32 / 255.0, 0xd6 as f32 / 255.0, 0xf5 as f32 / 255.0);

/// The Legacy Collection's selection gold — BNLC paints the picked
/// list row / focused thumbnail in this yellow with dark ink text.
/// Used by `widgets::list_item` for selected rows so "what you've
/// picked" reads in a different register from the cyan chrome.
pub const SELECT_YELLOW: iced::Color =
    iced::Color::from_rgb(0xff as f32 / 255.0, 0xd2 as f32 / 255.0, 0x3d as f32 / 255.0);

pub fn theme_for(config: &config::Config) -> Theme {
    // Tango palettes — these aren't tweaks of iced's stock Light /
    // Dark anymore. The dark variant is the Battle Network Legacy
    // Collection "PET" look: deep blue navy cyberworld base, cyan
    // chrome, gold selection. Light is the same identity under
    // daylight — pale ice blue with a deeper cyan that keeps
    // contrast — not a separate personality.
    match config.theme {
        config::ThemeMode::Light => Theme::custom(
            "Tango Light".to_string(),
            iced::theme::Palette {
                background: iced::Color::from_rgb(0xe9 as f32 / 255.0, 0xf2 as f32 / 255.0, 0xf9 as f32 / 255.0),
                text: iced::Color::from_rgb(0x10 as f32 / 255.0, 0x2a as f32 / 255.0, 0x3c as f32 / 255.0),
                // The PET cyan is too pale to read on ice white, so
                // light mode runs a deeper tone of the same hue.
                primary: LIGHT_CYAN,
                success: LIGHT_CYAN,
                warning: iced::Color::from_rgb(0xb7 as f32 / 255.0, 0x7e as f32 / 255.0, 0x33 as f32 / 255.0),
                danger: iced::Color::from_rgb(0xd1 as f32 / 255.0, 0x3a as f32 / 255.0, 0x3a as f32 / 255.0),
            },
        ),
        config::ThemeMode::Dark => Theme::custom(
            "Tango Dark".to_string(),
            iced::theme::Palette {
                // Deep blue navy — bluer than the old near-black so
                // the cyan chrome reads as light glowing off a
                // cyberworld, not neon on void.
                background: iced::Color::from_rgb(0x0a as f32 / 255.0, 0x1a as f32 / 255.0, 0x2c as f32 / 255.0),
                // Cyan-tinted off-white. The slight blue shift
                // keeps body copy from looking gray on the navy bg.
                text: iced::Color::from_rgb(0xe4 as f32 / 255.0, 0xf3 as f32 / 255.0, 0xfb as f32 / 255.0),
                primary: TANGO_CYAN,
                // Positive states ride the same cyan — BNLC has no
                // green anywhere, and neither do we anymore.
                success: TANGO_CYAN,
                warning: iced::Color::from_rgb(0xff as f32 / 255.0, 0xb5 as f32 / 255.0, 0x47 as f32 / 255.0),
                danger: iced::Color::from_rgb(0xff as f32 / 255.0, 0x52 as f32 / 255.0, 0x52 as f32 / 255.0),
            },
        ),
    }
}

/// Light mode's stand-in for [`TANGO_CYAN`]: same hue, pulled deep
/// enough to hold contrast on the ice-white page.
const LIGHT_CYAN: iced::Color =
    iced::Color::from_rgb(0x07 as f32 / 255.0, 0x82 as f32 / 255.0, 0xb4 as f32 / 255.0);

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

/// The trans flag's five stripes (blue / pink / white / pink / blue),
/// left→right, as linear-gradient stops — the symmetric mirror means it
/// reads the same flying either direction.
pub fn trans_flag_stops() -> [(f32, iced::Color); 5] {
    [
        (0.00, iced::Color::from_rgb8(0x5b, 0xce, 0xfa)), // light blue
        (0.25, iced::Color::from_rgb8(0xf5, 0xa9, 0xb8)), // pink
        (0.50, iced::Color::from_rgb8(0xff, 0xff, 0xff)), // white
        (0.75, iced::Color::from_rgb8(0xf5, 0xa9, 0xb8)), // pink
        (1.00, iced::Color::from_rgb8(0x5b, 0xce, 0xfa)), // light blue
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
