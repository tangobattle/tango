use fluent_templates::Loader;

use crate::{gui, i18n};

pub struct StealInputWindow;

impl StealInputWindow {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        language: &unic_langid::LanguageIdentifier,
        steal_input: &mut Option<gui::StealInputState>,
    ) {
        let mut steal_input_open = steal_input.is_some();
        if let Some(inner_response) = egui::Window::new("")
            .id(egui::Id::new("input-capture-window"))
            .open(&mut steal_input_open)
            .title_bar(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.with_layout(
                    egui::Layout::top_down_justified(egui::Align::Center),
                    |ui| {
                        egui::Frame::none()
                            .inner_margin(egui::style::Margin::symmetric(32.0, 16.0))
                            .show(ui, |ui| {
                                let userdata =
                                    if let Some(gui::StealInputState { userdata, .. }) =
                                        &steal_input
                                    {
                                        userdata
                                    } else {
                                        unreachable!();
                                    };

                                ui.label(
                                    egui::RichText::new(
                                        i18n::LOCALES
                                            .lookup_with_args(
                                                &language,
                                                "input-mapping.prompt",
                                                &std::collections::HashMap::from([(
                                                    "key",
                                                    i18n::LOCALES
                                                        .lookup(
                                                            &language,
                                                            userdata
                                                                .downcast_ref::<&str>()
                                                                .unwrap(),
                                                        )
                                                        .unwrap()
                                                        .into(),
                                                )]),
                                            )
                                            .unwrap(),
                                    )
                                    .size(32.0),
                                );
                            });
                    },
                );
            })
        {
            ctx.move_to_top(inner_response.response.layer_id);
        }
        if !steal_input_open {
            *steal_input = None;
        }
    }
}
