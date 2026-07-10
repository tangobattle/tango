//! Tango's iced `Theme` builder. The dark/light palettes are
//! custom — not iced's stock Light/Dark — so they live here
//! rather than picking from `iced::Theme::DARK` etc. Used by
//! `App::theme` (live theme for the window chrome) and by any
//! `view` function that needs to derive a theme-aware style
//! before iced has handed it the live `Theme` (e.g. markdown
//! link color in the About panel).

use crate::config;
use iced::Theme;

/// The default accent color — primary CTA buttons, the active tab
/// chip, panel frames, the cyberworld backdrop, markdown link color
/// in the About panel, etc. Same green the legacy egui app uses,
/// kept in one const so we never accidentally drift to a different
/// shade. (The Legacy Collection restyle briefly ran the PET cyan
/// here; the structure stayed, the color came back home — and now
/// also anchors `success` when the user picks a different
/// [`config::AccentColor`] for the chrome.)
pub const TANGO_GREEN: iced::Color =
    iced::Color::from_rgb(0x4c as f32 / 255.0, 0xaf as f32 / 255.0, 0x50 as f32 / 255.0);

/// The Legacy Collection's selection gold — BNLC paints the picked
/// list row / focused thumbnail in this yellow with dark ink text.
/// Used by `widgets::list_item` for selected rows so "what you've
/// picked" reads in a different register from the green chrome.
pub const SELECT_YELLOW: iced::Color =
    iced::Color::from_rgb(0xff as f32 / 255.0, 0xd2 as f32 / 255.0, 0x3d as f32 / 255.0);

/// The chrome color for a [`config::AccentColor`] choice, per theme
/// mode. Dark mode wants bright accents that glow against the
/// charcoal; light mode needs deeper shades of the same identity or
/// the frames/glows wash out on cream (the LC restyle learned this
/// with its pale PET cyan — hence the deeper `LIGHT_CYAN` twin).
/// Green is the exception: the tango green reads on both.
pub fn accent_color(accent: config::AccentColor, dark: bool) -> iced::Color {
    let rgb = match (accent, dark) {
        (config::AccentColor::TangoGreen, _) => (0x4c, 0xaf, 0x50),
        // The blue bomber's azure — bright enough to glow on
        // charcoal; cobalt in daylight.
        (config::AccentColor::MegaManBlue, true) => (0x4d, 0xa6, 0xff),
        (config::AccentColor::MegaManBlue, false) => (0x14, 0x5c, 0xc2),
        // Crimson, a step deeper than the danger red so alarms still
        // read a notch hotter than the chrome.
        (config::AccentColor::ProtoManRed, true) => (0xef, 0x40, 0x56),
        (config::AccentColor::ProtoManRed, false) => (0xb7, 0x1c, 0x30),
        (config::AccentColor::RollPink, true) => (0xff, 0x6e, 0xa8),
        (config::AccentColor::RollPink, false) => (0xc2, 0x2f, 0x6d),
        // GutsMan's metallic amber-gold — deeper than the selection
        // gold, which deliberately stays gold alongside it. Light
        // mode runs bronze (bright gold on cream has no contrast).
        (config::AccentColor::GutsManYellow, true) => (0xe6, 0xb4, 0x22),
        (config::AccentColor::GutsManYellow, false) => (0x96, 0x71, 0x18),
        // Bass's aura violet — bright enough to glow on charcoal;
        // royal purple in daylight for the same reason as the blues.
        (config::AccentColor::BassPurple, true) => (0xae, 0x6f, 0xf5),
        (config::AccentColor::BassPurple, false) => (0x6a, 0x35, 0xb5),
    };
    iced::Color::from_rgb(rgb.0 as f32 / 255.0, rgb.1 as f32 / 255.0, rgb.2 as f32 / 255.0)
}

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
                primary: accent_color(config.accent, false),
                // Success stays semantically green no matter what
                // the chrome runs — "it worked" shouldn't turn red
                // because the user picked Proto Red.
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
                primary: accent_color(config.accent, true),
                // See the Light arm — success is always green.
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

// The n/5 pattern spells out the six evenly-spaced stops; clippy's
// eq_op would flag the final 5.0/5.0.
#[allow(clippy::eq_op)]
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
