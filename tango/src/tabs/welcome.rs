use crate::i18n::{t, t_args};
use crate::tabs::settings::labeled;
use crate::{
    save_view, widgets, PRIMARY_PADDING, STANDARD_PADDING, SUPPORTED_LANGS,
    TEXT_BODY, TEXT_CAPTION, TEXT_DISPLAY, TEXT_TITLE,
};
use lucide_icons::Icon;
use iced::widget::{button, column, container, pick_list, row, text, text_input, Space};
use iced::{Alignment, Element, Fill, Length};
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone)]
pub enum Message {
    NicknameChanged(String),
    Continue,
    LanguageSelected(LanguageIdentifier),
    OpenRomsFolder,
    RescanRoms,
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
    pub fn finalize_nickname(&self) -> Option<String> {
        let trimmed = self.nickname_draft.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

/// Two-step welcome: drop ROMs in the data folder, then pick a
/// nickname. Mirrors the legacy app's `gui/welcome.rs` layout —
/// language picker at top, step rows with status glyphs, gated
/// Continue button.
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    state: &'a State,
    roms_count: usize,
    roms_path: &std::path::Path,
) -> Element<'a, Message> {
    let has_roms = roms_count > 0;

    // Language selector — lets the user switch before nickname so the
    // rest of the welcome flow shows in the language they picked.
    // Same `LanguageChoice` wrapper the settings picker uses so the
    // dropdown shows each language's endonym (e.g. "日本語") rather
    // than its locale code.
    let lang_options: Vec<crate::i18n::LanguageChoice> = SUPPORTED_LANGS
        .iter()
        .map(|id| crate::i18n::LanguageChoice::new(id.clone()))
        .collect();
    let lang_selected = lang_options.iter().find(|c| &c.id == lang).cloned();
    let lang_picker = pick_list(lang_options, lang_selected, |c: crate::i18n::LanguageChoice| {
        Message::LanguageSelected(c.id)
    })
    .padding(STANDARD_PADDING);

    let step_marker = |done: bool| -> Icon {
        if done {
            Icon::Check
        } else {
            Icon::RefreshCw
        }
    };

    // Step 1 — ROMs.
    let mut roms_block = column![
        row![
            step_marker(has_roms).widget().size(16.0),
            text(t(lang, "welcome-step-roms")).size(TEXT_TITLE),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        text(t(lang, "welcome-step-roms-description"))
            .size(TEXT_CAPTION)
            .style(save_view::muted_text_style),
        text(roms_path.display().to_string())
            .size(TEXT_CAPTION)
            .font(iced::Font::MONOSPACE),
        row![
            widgets::labeled_icon_button(
                Icon::Folder,
                t(lang, "welcome-open-folder"),
                Message::OpenRomsFolder,
                STANDARD_PADDING,
                widgets::neutral,
            ),
            widgets::labeled_icon_button(
                Icon::RefreshCw,
                t(lang, "rescan"),
                Message::RescanRoms,
                STANDARD_PADDING,
                widgets::neutral,
            ),
        ]
        .spacing(8),
    ]
    .spacing(6);
    if has_roms {
        roms_block = roms_block.push(
            text(t_args(
                lang,
                "welcome-step-roms-detected",
                &[("count", (roms_count as i64).into())],
            ))
            .size(TEXT_CAPTION)
            .style(|theme: &iced::Theme| iced::widget::text::Style {
                color: Some(theme.palette().primary),
            }),
        );
    }

    // Step 2 — nickname. Gated until at least one ROM is detected.
    let can_continue = has_roms && !state.nickname_draft.trim().is_empty();
    let mut continue_btn = button(text(t(lang, "welcome-continue"))).padding(PRIMARY_PADDING);
    if can_continue {
        continue_btn = continue_btn.style(button::primary).on_press(Message::Continue);
    } else {
        continue_btn = continue_btn.style(widgets::neutral);
    }

    let mut nickname_block = column![
        row![
            step_marker(!state.nickname_draft.trim().is_empty()).widget().size(16.0),
            text(t(lang, "welcome-step-nickname")).size(TEXT_TITLE),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        text(t(lang, "welcome-step-nickname-description"))
            .size(TEXT_CAPTION)
            .style(save_view::muted_text_style),
        labeled::<Message>(
            t(lang, "settings-nickname"),
            text_input("", &state.nickname_draft)
                .on_input(Message::NicknameChanged)
                .on_submit(Message::Continue)
                
                .padding(STANDARD_PADDING)
                .width(Length::Fixed(280.0)),
        ),
    ]
    .spacing(6);
    if !has_roms {
        nickname_block = nickname_block.push(
            text(t(lang, "welcome-roms-needed"))
                .size(TEXT_CAPTION)
                .style(save_view::muted_text_style),
        );
    }
    nickname_block = nickname_block.push(Space::new().height(8)).push(continue_btn);

    container(
        column![
            row![
                text(t(lang, "welcome-title")).size(TEXT_DISPLAY),
                Space::new().width(Fill),
                lang_picker
            ]
            .align_y(Alignment::Center),
            text(t(lang, "welcome-subtitle"))
                .size(TEXT_BODY)
                .style(save_view::muted_text_style),
            Space::new().height(16),
            roms_block,
            Space::new().height(20),
            nickname_block,
        ]
        .spacing(8)
        .align_x(Alignment::Start)
        .padding(24)
        .max_width(560),
    )
    .center(Fill)
    .into()
}
