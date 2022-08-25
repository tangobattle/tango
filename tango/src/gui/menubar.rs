use fluent_templates::Loader;

use crate::i18n;

use super::{play_window, settings_window};

pub struct Menubar {}

impl Menubar {
    pub fn new() -> Self {
        Menubar {}
    }

    pub fn show(
        &self,
        ctx: &egui::Context,
        lang: &unic_langid::LanguageIdentifier,
        last_cursor_activity_time: &Option<std::time::Instant>,
        show_play: &mut Option<play_window::State>,
        show_settings: &mut Option<settings_window::State>,
        show_about: &mut bool,
    ) {
        egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        show_play.is_some(),
                        format!("🎮 {}", i18n::LOCALES.lookup(&lang, "play").unwrap()),
                    )
                    .clicked()
                {
                    *show_play = if show_play.is_some() {
                        None
                    } else {
                        Some(play_window::State::new())
                    };
                }

                if ui
                    .selectable_label(
                        show_settings.is_some(),
                        format!("⚙️ {}", i18n::LOCALES.lookup(&lang, "settings").unwrap()),
                    )
                    .clicked()
                {
                    *show_settings = if show_settings.is_some() {
                        None
                    } else {
                        Some(settings_window::State::new())
                    };
                }

                if ui
                    .selectable_label(
                        *show_about,
                        format!("❓ {}", i18n::LOCALES.lookup(&lang, "about").unwrap()),
                    )
                    .clicked()
                {
                    *show_about = !*show_about;
                }
            })
        });
    }
}
