//! This layer's own widgets, over the shared toolkit in
//! [`tango_ui::widgets`] — re-exported here, so `crate::widgets::*`
//! covers both without call sites caring which side of the boundary a
//! widget lives on. Only what the save editor alone wears is defined
//! here: its sub-tab pill and the data tables' zebra rows.

pub use tango_ui::widgets::*;

use iced::{Element, Theme};
use lucide_icons::Icon;

/// Compact tab pill used by sub-navs (save_view's
/// Cover/Navi/Folder/Patch Cards/Auto Battle Data strip, etc).
/// Body-text size, modest padding — meant to sit inside a pane
/// without competing with the global top nav.
pub fn tab_button<'a, M: Clone + 'a>(icon: Icon, label: String, msg: M, active: bool) -> Element<'a, M> {
    pill_tab(icon, Some(label), msg, active, false)
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
