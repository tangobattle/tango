//! Small iced widget helpers: icon-buttons (with tooltips), tab
//! buttons, button styles (`neutral`, `list_item`). Icon glyphs
//! come straight from the `lucide-icons` crate — call sites pass
//! `Icon::Foo` directly.

use iced::widget::{button, container, row, text, tooltip};
use iced::{Alignment, Element, Theme};
use lucide_icons::Icon;

/// Icon-only button for low-emphasis toolbar actions (rescan,
/// copy, open-folder, etc.). Uses [`neutral`] — a soft, theme-
/// aware style that doesn't compete with primary CTAs in the
/// same row. The plain-text label is exposed as a hover tooltip.
pub fn icon_button<'a, M: Clone + 'a>(icon: Icon, label: String, msg: M, padding: [f32; 2]) -> Element<'a, M> {
    icon_button_styled(icon, label, Some(msg), padding, neutral)
}

/// `icon_button` with the on_press wrapped in an Option so callers
/// can render a disabled (greyed-out, no on_press) variant without
/// duplicating the chrome.
pub fn icon_button_maybe<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    msg: Option<M>,
    padding: [f32; 2],
) -> Element<'a, M> {
    icon_button_styled(icon, label, msg, padding, neutral)
}

/// List-item button style for selectable rows (patches list,
/// replays list). Selected row uses the bright primary fill;
/// inactive rows are transparent with a faint hover tone.
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
/// tone), brightens on hover, dims when disabled.
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
    icon: Icon,
    label: String,
    msg: Option<M>,
    padding: [f32; 2],
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, M> {
    let mut btn = button(icon.widget()).padding(padding).style(style);
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

/// Icon-plus-label button. Icon and label use distinct fonts
/// (icon = lucide, label = app default), laid out as a row.
pub fn labeled_icon_button<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    msg: M,
    padding: [f32; 2],
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, M> {
    button(row![icon.widget(), text(label)].spacing(8).align_y(Alignment::Center))
        .padding(padding)
        .style(style)
        .on_press(msg)
        .into()
}

/// Flat compact tab — icon + label, transparent until hovered,
/// underlined with a 2 px colored bar when active.
pub fn tab_button<'a, M: Clone + 'a>(icon: Icon, label: String, msg: M, active: bool) -> Element<'a, M> {
    use iced::widget::{stack, Space};
    let btn = button(row![icon.widget(), text(label)].spacing(6).align_y(Alignment::Center))
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
