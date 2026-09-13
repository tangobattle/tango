use iced::{widget, Alignment, Color, Length, Theme};
use tango_script::ui::{
    canvas::{Font, TextSize, TextSizeName},
    style::{
        Align, Appearance, Background, Color as ScriptColor, Cursor, Padding, Size, SizeName, ThemeColor, Tone,
        ToneName,
    },
};
use tango_ui::{style, widgets};

pub(crate) fn length(size: Size) -> Length {
    match size {
        Size::Named(SizeName::Shrink) => Length::Shrink,
        Size::Named(SizeName::Fill) => Length::Fill,
        Size::Pixels(n) => Length::Fixed(n),
        Size::Portion { portion } => Length::FillPortion(portion),
    }
}

pub(crate) fn alignment(align: Align) -> Alignment {
    match align {
        Align::Start => Alignment::Start,
        Align::Center => Alignment::Center,
        Align::End => Alignment::End,
    }
}

pub(crate) fn cursor(cursor: Cursor) -> iced::mouse::Interaction {
    use iced::mouse::Interaction;
    match cursor {
        Cursor::Default => Interaction::Idle,
        Cursor::Hidden => Interaction::Hidden,
        Cursor::ContextMenu => Interaction::ContextMenu,
        Cursor::Help => Interaction::Help,
        Cursor::Pointer => Interaction::Pointer,
        Cursor::Progress => Interaction::Progress,
        Cursor::Wait => Interaction::Wait,
        Cursor::Cell => Interaction::Cell,
        Cursor::Crosshair => Interaction::Crosshair,
        Cursor::Text => Interaction::Text,
        Cursor::Alias => Interaction::Alias,
        Cursor::Copy => Interaction::Copy,
        Cursor::Move => Interaction::Move,
        Cursor::NoDrop => Interaction::NoDrop,
        Cursor::NotAllowed => Interaction::NotAllowed,
        Cursor::Grab => Interaction::Grab,
        Cursor::Grabbing => Interaction::Grabbing,
        Cursor::ResizeHorizontal => Interaction::ResizingHorizontally,
        Cursor::ResizeVertical => Interaction::ResizingVertically,
        Cursor::ResizeDiagonalUp => Interaction::ResizingDiagonallyUp,
        Cursor::ResizeDiagonalDown => Interaction::ResizingDiagonallyDown,
        Cursor::ResizeColumn => Interaction::ResizingColumn,
        Cursor::ResizeRow => Interaction::ResizingRow,
        Cursor::AllScroll => Interaction::AllScroll,
        Cursor::ZoomIn => Interaction::ZoomIn,
        Cursor::ZoomOut => Interaction::ZoomOut,
    }
}

pub(crate) fn text_size(size: TextSize) -> f32 {
    match size {
        TextSize::Pixels(n) => n,
        TextSize::Named(TextSizeName::Title) => style::TEXT_TITLE,
        TextSize::Named(TextSizeName::Heading) => style::TEXT_HEADING,
        TextSize::Named(TextSizeName::Body) => style::TEXT_BODY,
        TextSize::Named(TextSizeName::Caption) => style::TEXT_CAPTION,
    }
}

pub(crate) fn font(role: Font) -> iced::Font {
    match role {
        Font::Normal => iced::Font::DEFAULT,
        Font::Mono => style::MONOSPACE_FONT,
        Font::Icons => iced::Font::with_name("lucide"),
    }
}

pub(crate) fn color(theme: &Theme, color: ScriptColor) -> Color {
    let palette = theme.extended_palette();
    match color {
        ScriptColor::Rgba([r, g, b, a]) => Color { r, g, b, a },
        ScriptColor::ThemeAlpha { theme: role, alpha } => Color {
            a: alpha,
            ..self::color(theme, ScriptColor::Theme(role))
        },
        ScriptColor::ThemeTint {
            theme: role,
            tint: [r, g, b, a],
            mix,
        } => {
            let base = self::color(theme, ScriptColor::Theme(role));
            let tinted = widgets::mix(base, Color { r, g, b, a }, mix);
            Color {
                a: base.a * (1.0 - mix) + a * mix,
                ..tinted
            }
        }
        ScriptColor::Theme(role) => match role {
            ThemeColor::Text => palette.background.base.text,
            ThemeColor::Muted => widgets::muted_color(theme),
            ThemeColor::Primary => palette.primary.base.color,
            ThemeColor::Danger => palette.danger.base.color,
            ThemeColor::DangerStrong => palette.danger.strong.color,
            ThemeColor::Success => palette.success.base.color,
            ThemeColor::Warning => palette.warning.base.color,
            ThemeColor::Background => palette.background.base.color,
            ThemeColor::Plate => widgets::plate_color(theme),
            ThemeColor::Border => palette.background.strong.color,
            ThemeColor::Transparent => Color::TRANSPARENT,
        },
    }
}

pub(crate) fn button(theme: &Theme, status: widget::button::Status, tone: Tone) -> widget::button::Style {
    match tone {
        Tone::Named(ToneName::Primary) => widgets::primary_button(theme, status),
        Tone::Named(ToneName::Danger) => widgets::danger_button(theme, status),
        Tone::Named(ToneName::Transparent) => widgets::flat(theme, status),
        Tone::Named(ToneName::Selected) => widgets::pill_tab_style(true)(theme, status),
        Tone::Tab { tab } => widgets::pill_tab_style(tab)(theme, status),
        Tone::Row { row, selected } => widgets::list_item(selected, row.saturating_sub(1) as usize)(theme, status),
        Tone::Tinted { tint } => widgets::tinted_button(theme, status, color(theme, tint)),
        _ => widgets::neutral(theme, status),
    }
}

pub(crate) fn container(theme: &Theme, tone: Tone) -> widget::container::Style {
    match tone {
        Tone::Named(ToneName::Card) => widgets::pane(theme),
        Tone::Named(ToneName::Tooltip) => widgets::tooltip_chrome(theme),
        Tone::Named(ToneName::Plate) => widget::container::Style::default().background(widgets::plate_color(theme)),
        Tone::Named(ToneName::Selected) => {
            widget::container::Style::default().background(theme.extended_palette().primary.weak.color)
        }
        Tone::Row { row, selected: false } => widgets::zebra_row(row.saturating_sub(1) as usize)(theme),
        Tone::Row { row, selected: true } => {
            let base = widgets::list_item(true, row.saturating_sub(1) as usize)(theme, widget::button::Status::Active);
            widget::container::Style {
                background: base.background,
                text_color: Some(base.text_color),
                border: base.border,
                shadow: base.shadow,
                ..Default::default()
            }
        }
        Tone::Tinted { tint } => widget::container::Style::default().background(color(theme, tint)),
        _ => widget::container::Style::default(),
    }
}

pub(crate) fn padding(value: Padding) -> iced::Padding {
    let [top, right, bottom, left] = value.sides();
    iced::Padding {
        top,
        right,
        bottom,
        left,
    }
}

fn border(theme: &Theme, border: &mut iced::Border, appearance: &Appearance) {
    if let Some(color) = appearance.border_color {
        border.color = self::color(theme, color);
    }
    if let Some(width) = appearance.border_width {
        border.width = width;
    }
    if let Some(radius) = appearance.radius {
        border.radius = radius.into();
    }
}

fn background(theme: &Theme, background: &Background) -> iced::Background {
    match background {
        Background::Solid(color) => self::color(theme, *color).into(),
        Background::Linear { angle, stops } => {
            let mut gradient = iced::gradient::Linear::new(*angle);
            for stop in stops {
                gradient = gradient.add_stop(stop.offset, color(theme, stop.color));
            }
            iced::Background::Gradient(iced::Gradient::Linear(gradient))
        }
    }
}

pub(crate) fn container_appearance(
    theme: &Theme,
    mut style: widget::container::Style,
    appearance: &Appearance,
) -> widget::container::Style {
    if let Some(value) = &appearance.background {
        style.background = Some(background(theme, value));
    }
    if let Some(text) = appearance.text_color {
        style.text_color = Some(color(theme, text));
    }
    border(theme, &mut style.border, appearance);
    if let Some(shadow) = appearance.shadow {
        style.shadow = iced::Shadow {
            color: color(theme, shadow.color),
            offset: iced::Vector::new(shadow.offset[0], shadow.offset[1]),
            blur_radius: shadow.blur,
        };
    }
    style
}

pub(crate) fn button_appearance(
    theme: &Theme,
    mut style: widget::button::Style,
    appearance: &Appearance,
) -> widget::button::Style {
    if let Some(value) = &appearance.background {
        style.background = Some(background(theme, value));
    }
    if let Some(text) = appearance.text_color {
        style.text_color = color(theme, text);
    }
    border(theme, &mut style.border, appearance);
    if let Some(shadow) = appearance.shadow {
        style.shadow = iced::Shadow {
            color: color(theme, shadow.color),
            offset: iced::Vector::new(shadow.offset[0], shadow.offset[1]),
            blur_radius: shadow.blur,
        };
    }
    style
}
