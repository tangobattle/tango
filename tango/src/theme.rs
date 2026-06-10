//! Tango's iced `Theme` builder. The dark/light palettes are
//! custom — not iced's stock Light/Dark — so they live here
//! rather than picking from `iced::Theme::DARK` etc. Used by
//! `App::theme` (live theme for the window chrome) and by any
//! `view` function that needs to derive a theme-aware style
//! before iced has handed it the live `Theme` (e.g. markdown
//! link color in the About panel).

use crate::config;
use iced::Theme;

/// The accent color used across the app — selection highlights,
/// primary CTA buttons, the active tab underline, markdown link
/// color in the About panel, etc. Same green the legacy egui
/// app uses, kept in one const so we never accidentally drift to
/// a different shade.
pub const TANGO_GREEN: iced::Color =
    iced::Color::from_rgb(0x4c as f32 / 255.0, 0xaf as f32 / 255.0, 0x50 as f32 / 255.0);

pub fn theme_for(config: &config::Config) -> Theme {
    // Tango palettes — these aren't tweaks of iced's stock Light /
    // Dark anymore. The dark variant is a deep navy "cyberworld"
    // base (think MMBN's PET screens / the legacy egui theme's
    // accent) so panels read as game chrome rather than a generic
    // desktop app. Light is a warm cream + slate set tuned to feel
    // like the same UI under daylight, not a separate identity.
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
                // Deep navy black — darker than stock iced Dark
                // (0x2b2d31) so the neon green primary actually
                // glows against it instead of competing.
                background: iced::Color::from_rgb(0x0b as f32 / 255.0, 0x12 as f32 / 255.0, 0x1c as f32 / 255.0),
                // Cyan-tinted off-white. The slight blue shift
                // keeps body copy from looking gray on the navy bg.
                text: iced::Color::from_rgb(0xe4 as f32 / 255.0, 0xf3 as f32 / 255.0, 0xfb as f32 / 255.0),
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
