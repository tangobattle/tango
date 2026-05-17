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

/// Welcome-screen state: just the in-progress nickname draft until
/// the user hits Continue, at which point App copies it into
/// `config.nickname` and the welcome screen is never shown again.
#[derive(Default)]
pub struct State {
    pub nickname_draft: String,
}

impl State {
    pub fn from_nickname(nickname: Option<&str>) -> Self {
        Self {
            nickname_draft: nickname.unwrap_or_default().to_string(),
        }
    }

    /// Returns Some(trimmed_nickname) if the user finalized the
    /// welcome step (clicked Continue or pressed Enter on a non-empty
    /// input). The caller is expected to write it to `config.nickname`.
    pub fn update(&mut self, msg: Message) -> Option<String> {
        match msg {
            Message::NicknameChanged(s) => {
                self.nickname_draft = s;
                None
            }
            Message::Continue => {
                let trimmed = self.nickname_draft.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
        }
    }
}

pub fn view<'a>(lang: &'a LanguageIdentifier, state: &'a State) -> Element<'a, Message> {
    let draft = &state.nickname_draft;
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
