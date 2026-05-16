use crate::i18n::t;
use crate::tabs::settings::labeled;
use crate::{save_view, PRIMARY_PADDING, PRIMARY_TEXT_SIZE};
use iced::widget::{button, column, container, text, text_input, Space};
use iced::{Alignment, Element, Fill, Length};
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone)]
pub enum Message {
    NicknameChanged(String),
    Continue,
}

pub fn welcome_view(lang: &LanguageIdentifier, draft: &str) -> Element<'static, Message> {
    let can_continue = !draft.trim().is_empty();
    let mut continue_btn = button(text(t(lang, "welcome-continue")).size(PRIMARY_TEXT_SIZE))
        .padding(PRIMARY_PADDING)
        .style(button::primary);
    if can_continue {
        continue_btn = continue_btn.on_press(Message::Continue);
    }

    container(
        column![
            text(t(lang, "welcome-title")).size(28),
            text(t(lang, "welcome-subtitle"))
                .size(13)
                .style(save_view::muted_text_style),
            Space::with_height(16),
            labeled::<Message>(
                t(lang, "settings-nickname"),
                text_input("", draft)
                    .on_input(Message::NicknameChanged)
                    .on_submit(Message::Continue)
                    .padding(10)
                    .width(Length::Fixed(280.0)),
            ),
            Space::with_height(8),
            continue_btn,
        ]
        .spacing(8)
        .align_x(Alignment::Start)
        .padding(24)
        .max_width(420),
    )
    .center(Fill)
    .into()
}
