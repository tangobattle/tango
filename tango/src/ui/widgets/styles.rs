//! Standalone style functions for stock iced widgets: the window close
//! button, sliders, progress bars, disabled pickers, console keys.

use super::*;

/// The fullscreen top bar's app-close X — window chrome, not a
/// toolbar action. Borderless and muted at rest so it doesn't
/// compete with the nav pills, flipping to a solid danger plate
/// with a white glyph on hover: the universal titlebar-close
/// idiom, so "this closes the whole app" lands before the tooltip
/// does.
pub fn window_close(theme: &Theme, status: button::Status) -> button::Style {
    let danger = theme.palette().danger;
    let (bg, text_color) = match status {
        button::Status::Hovered => (danger, iced::Color::WHITE),
        button::Status::Pressed => (mix(danger, iced::Color::BLACK, 0.15), iced::Color::WHITE),
        button::Status::Active | button::Status::Disabled => (iced::Color::TRANSPARENT, muted_color(theme)),
    };
    button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color,
        border: iced::Border {
            color: iced::Color::TRANSPARENT,
            width: 0.0,
            radius: tech_radius(8.0),
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

/// The molded-plastic fill the drawn-GBA console keys share (and the
/// D-pad hub) — a step above the surrounding plate so keys read as
/// raised. Used by the settings input pane's console and the replay
/// input display, which mirrors its layout.
pub fn gba_key_plate(theme: &Theme) -> iced::Color {
    let p = theme.extended_palette();
    let bg = theme.palette().background;
    if p.is_dark {
        mix(bg, theme.palette().text, 0.16)
    } else {
        mix(bg, iced::Color::WHITE, 0.65)
    }
}

/// Container style that mimics a disabled `chunky_pick_list`. iced
/// 0.14's `pick_list::Status` has no Disabled variant, so we render
/// a styled `container` instead of the picker when the control isn't
/// usable. Same recipe as `tinted_button`'s Disabled branch (flat
/// desaturated plate + dim text + dim border) so disabled dropdowns
/// and disabled buttons read as the same family.
pub fn disabled_pick_list_style(theme: &Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let bg = theme.palette().background;
    let text = theme.palette().text;
    let dim = if p.is_dark {
        mix(bg, plate_lift(theme), 0.11)
    } else {
        mix(bg, text, 0.08)
    };
    iced::widget::container::Style {
        text_color: Some(iced::Color { a: 0.35, ..text }),
        background: Some(iced::Background::Color(dim)),
        border: iced::Border {
            radius: tech_radius(10.0),
            width: 1.0,
            color: iced::Color {
                a: 0.15,
                ..p.background.strong.color
            },
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

/// Drop-in stand-in for a `chunky_pick_list` when the choice isn't
/// available. Pads + radii match the live picker so the layout
/// doesn't shift when toggling between enabled/disabled states.
pub fn disabled_pick_list<'a, M: 'a>(label: impl Into<String>) -> iced::widget::Container<'a, M> {
    iced::widget::container(iced::widget::text(label.into()))
        .padding(crate::ui::style::STANDARD_PADDING)
        .style(disabled_pick_list_style)
}

/// Chunky slider matching the button bevel: a thicker rounded rail
/// whose filled side runs primary (brightening on hover / drag) and
/// whose empty side is the neutral plate, plus a circular handle
/// with the same white-tinted border as the CTA buttons so it reads
/// as a physical thumb rather than iced's flat default dot.
pub fn chunky_slider(theme: &Theme, status: iced::widget::slider::Status) -> iced::widget::slider::Style {
    use iced::widget::slider::{Handle, HandleShape, Rail, Status, Style};
    let p = theme.extended_palette();
    let primary = theme.palette().primary;
    let bg = theme.palette().background;
    let text = theme.palette().text;
    // Empty track: same plate recipe as the neutral button so the
    // rail reads as part of the same widget family.
    let track = if p.is_dark {
        mix(bg, plate_lift(theme), 0.18)
    } else {
        mix(bg, text, 0.18)
    };
    let (fill, grip, radius) = match status {
        Status::Hovered => (
            mix(primary, iced::Color::WHITE, 0.10),
            mix(primary, iced::Color::WHITE, 0.18),
            9.0,
        ),
        Status::Dragged => (
            mix(primary, iced::Color::WHITE, 0.18),
            mix(primary, iced::Color::WHITE, 0.28),
            9.0,
        ),
        Status::Active => (primary, primary, 8.0),
    };
    Style {
        rail: Rail {
            backgrounds: (iced::Background::Color(fill), iced::Background::Color(track)),
            width: 6.0,
            border: iced::Border {
                radius: 3.0.into(),
                width: 0.0,
                color: iced::Color::TRANSPARENT,
            },
        },
        handle: Handle {
            shape: HandleShape::Circle { radius },
            background: iced::Background::Color(grip),
            border_width: 2.0,
            border_color: mix(primary, iced::Color::WHITE, 0.35),
        },
    }
}

/// Slim progress bar: faint text-tinted track + primary fill with
/// pill-rounded ends. Pair with `.girth(Length::Fixed(4.0))` for
/// the thin "loading strip" look used by the replay exporter.
pub fn slim_progress_bar(theme: &Theme) -> iced::widget::progress_bar::Style {
    let text = theme.palette().text;
    iced::widget::progress_bar::Style {
        background: iced::Background::Color(iced::Color { a: 0.12, ..text }),
        bar: iced::Background::Color(theme.palette().primary),
        border: iced::Border {
            radius: 999.0.into(),
            width: 0.0,
            color: iced::Color::TRANSPARENT,
        },
    }
}
