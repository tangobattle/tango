//! This layer's own widgets, over the shared toolkit in
//! [`tango_ui::widgets`] — re-exported here, so `crate::widgets::*`
//! covers both without call sites caring which side of the boundary a
//! widget lives on. Only what the save editor alone wears is defined
//! here: its sub-tab pill and the data tables' zebra rows.

pub use tango_ui::widgets::*;

use iced::{Element, Theme};
use lucide_icons::Icon;

/// Plain danger-red bullet used beside localized legality errors.
pub fn error_dot<'a, M: 'a>() -> Element<'a, M> {
    iced::widget::text("•")
    .style(|theme: &Theme| iced::widget::text::Style {
        color: Some(theme.palette().danger),
    })
    .into()
}

/// Compact tab pill used by sub-navs (save_view's
/// Cover/Navi/Folder/Patch Cards/Auto Battle Data strip, etc).
/// Body-text size, modest padding — meant to sit inside a pane
/// without competing with the global top nav. A legality error makes
/// the label red and exposes every localized error in a whole-pill hover
/// tooltip; no separate warning glyph competes with the tab icon.
pub fn tab_button<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    msg: M,
    active: bool,
    legality_errors: Option<&[String]>,
) -> Element<'a, M> {
    use iced::widget::{button, column, container, row, text, tooltip};
    use iced::{Alignment, Length};

    let mut label = text(label);
    if legality_errors.is_some() {
        label = label.style(|theme: &Theme| iced::widget::text::Style {
            color: Some(theme.palette().danger),
        });
    }
    let content = row![icon.widget().size(crate::style::TEXT_BODY), label]
        .spacing(8)
        .align_y(Alignment::Center);
    let button = button(content)
        .padding([6.0, 14.0])
        .style(pill_tab_style(active))
        .on_press(msg);

    let Some(errors) = legality_errors.filter(|errors| !errors.is_empty()) else {
        return button.into();
    };
    let mut error_list = column![].spacing(5);
    for error in errors {
        error_list = error_list.push(
            row![
                error_dot::<M>(),
                text(error.clone())
                    .size(crate::style::TEXT_CAPTION)
                    .width(Length::Fixed(360.0)),
            ]
            .spacing(6)
            .align_y(Alignment::Start),
        );
    }
    tooltip(
        button,
        container(error_list).padding(8).style(tooltip_chrome),
        tooltip::Position::FollowCursor,
    )
    .gap(8)
    .into()
}

/// Zebra row style for data tables. Odd rows get a faint text-
/// tinted wash (alpha 0.05 dark / 0.04 light); even rows are
/// transparent and show the pane plate. Flat — no rounded corners
/// — since rows sit flush against the pane edges and rounded
/// per-row corners look like accidental indents.
pub fn zebra_row(idx: usize) -> impl Fn(&Theme) -> iced::widget::container::Style {
    move |theme: &Theme| {
        let p = theme.extended_palette();
        let text = theme.palette().text;
        let stripe = if idx % 2 == 1 {
            Some(iced::Background::Color(iced::Color {
                a: if p.is_dark { 0.05 } else { 0.04 },
                ..text
            }))
        } else {
            None
        };
        iced::widget::container::Style {
            background: stripe,
            text_color: Some(text),
            ..Default::default()
        }
    }
}

/// Normal library-row chrome with danger-red text for over-limit chips that
/// remain pressable. Full-folder choices use the separate disabled gray wash.
pub fn danger_text_list_item(
    idx: usize,
) -> impl Fn(&Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |theme: &Theme, status: iced::widget::button::Status| {
        let mut style = tango_ui::widgets::list_item(false, idx)(theme, status);
        style.text_color = theme.palette().danger;
        style
    }
}
