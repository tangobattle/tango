//! Trill's iced `Theme` builder. The dark/light palettes are
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
pub const TRILL_YELLOW: iced::Color =
    iced::Color::from_rgb(0xff as f32 / 255.0, 0xf2 as f32 / 255.0, 0xa7 as f32 / 255.0);

pub const TRILL_YELLOW_DARK: iced::Color =
    iced::Color::from_rgb(0xdc as f32 / 255.0, 0xb2 as f32 / 255.0, 0x7a as f32 / 255.0);

pub fn theme_for(config: &config::Config) -> Theme {
    match config.theme {
        config::ThemeMode::Light => Theme::custom(
            "Trill Light".to_string(),
            iced::theme::Palette {
                background: iced::Color::from_rgb(0xf3 as f32 / 255.0, 0xee as f32 / 255.0, 0xdc as f32 / 255.0),
                text: iced::Color::from_rgb(0x14 as f32 / 255.0, 0x22 as f32 / 255.0, 0x34 as f32 / 255.0),
                primary: TRILL_YELLOW,
                success: TRILL_YELLOW,
                warning: iced::Color::from_rgb(0xb7 as f32 / 255.0, 0x7e as f32 / 255.0, 0x33 as f32 / 255.0),
                danger: iced::Color::from_rgb(0xd1 as f32 / 255.0, 0x3a as f32 / 255.0, 0x3a as f32 / 255.0),
            },
        ),
        config::ThemeMode::Dark => Theme::custom(
            "Trill Dark".to_string(),
            iced::theme::Palette {
                // Deep navy black — darker than stock iced Dark
                // (0x2b2d31) so the neon green primary actually
                // glows against it instead of competing.
                background: iced::Color::from_rgb(0x0b as f32 / 255.0, 0x12 as f32 / 255.0, 0x1c as f32 / 255.0),
                // Cyan-tinted off-white. The slight blue shift
                // keeps body copy from looking gray on the navy bg.
                text: iced::Color::from_rgb(0xe4 as f32 / 255.0, 0xf3 as f32 / 255.0, 0xfb as f32 / 255.0),
                primary: TRILL_YELLOW_DARK,
                success: TRILL_YELLOW_DARK,
                warning: iced::Color::from_rgb(0xff as f32 / 255.0, 0xb5 as f32 / 255.0, 0x47 as f32 / 255.0),
                danger: iced::Color::from_rgb(0xff as f32 / 255.0, 0x52 as f32 / 255.0, 0x52 as f32 / 255.0),
            },
        ),
    }
}
