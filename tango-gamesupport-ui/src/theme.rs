//! Theme constants and decorations the shared widgets draw with. The
//! app's `Theme` *builder* (palettes, accent-color choices, markdown
//! style) stays in tango — it reads the user config — and re-exports
//! these alongside it as `crate::ui::theme::*`.

/// The default accent color — primary CTA buttons, the active tab
/// chip, panel frames, the cyberworld backdrop, markdown link color
/// in the About panel, etc. Same green the legacy egui app uses,
/// kept in one const so we never accidentally drift to a different
/// shade. (The Legacy Collection restyle briefly ran the PET cyan
/// here; the structure stayed, the color came back home — and now
/// also anchors `success` when the user picks a different accent
/// for the chrome.)
pub const TANGO_GREEN: iced::Color =
    iced::Color::from_rgb(0x4c as f32 / 255.0, 0xaf as f32 / 255.0, 0x50 as f32 / 255.0);

/// The Legacy Collection's selection gold — BNLC paints the picked
/// list row / focused thumbnail in this yellow with dark ink text.
/// Used by `widgets::list_item` for selected rows so "what you've
/// picked" reads in a different register from the green chrome.
pub const SELECT_YELLOW: iced::Color =
    iced::Color::from_rgb(0xff as f32 / 255.0, 0xd2 as f32 / 255.0, 0x3d as f32 / 255.0);

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
