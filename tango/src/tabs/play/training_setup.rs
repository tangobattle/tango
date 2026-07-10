//! Training-mode setup for the Play tab: the modal opened by the save
//! view's Training button, where the user picks the match type, their
//! side, and an optional seed before launching a
//! [`crate::session::training::TrainingSession`]. The scriptless dummy
//! mirrors the local save; a script — picked live from the training
//! bar — defines the save itself via `on_setup` (from a zeroed image,
//! deterministically), so neither save nor script is chosen here. Pure setup UI; the launch itself
//! rides [`Effect::StartTraining`] up to the App.

use super::*;
// Explicit so these win over iced's prelude macros (same dance as mod.rs).
use sweeten::widget::{column, row, text_input};

/// Draft state for the setup modal. `None` = modal closed.
#[derive(Clone, Debug)]
pub struct TrainingSetup {
    pub match_type: (u8, u8),
    /// Which side the user plays: 0 = P1, 1 = P2.
    pub side: u8,
    /// Free-text seed; empty = random. The same text always produces the
    /// same chip draws, so drills are repeatable and shareable.
    pub seed: String,
}

impl TrainingSetup {
    /// Fresh draft for the given selection: mirror match against the
    /// same save, defaulting to Triple where the game has it (the same
    /// default the netplay lobby applies), P1, random seed.
    pub fn for_selection(loaded: &selection::Loaded) -> Self {
        let mt_table = game::from_gamedb_entry(loaded.game)
            .map(|g| g.match_types)
            .unwrap_or(&[]);
        let match_type = if mt_table.get(1).copied().unwrap_or(0) > 0 {
            (1, 0)
        } else {
            (0, 0)
        };
        Self {
            match_type,
            side: 0,
            seed: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TrainingSetupMessage {
    MatchTypeSelected((u8, u8)),
    SideSelected(u8),
    SeedChanged(String),
    Start,
    Cancel,
}

impl State {
    pub(super) fn update_training_setup(&mut self, msg: TrainingSetupMessage) -> Option<Effect> {
        match msg {
            TrainingSetupMessage::MatchTypeSelected(mt) => {
                if let Some(setup) = self.training_setup.as_mut() {
                    setup.match_type = mt;
                }
                None
            }
            TrainingSetupMessage::SideSelected(side) => {
                if let Some(setup) = self.training_setup.as_mut() {
                    setup.side = side.min(1);
                }
                None
            }
            TrainingSetupMessage::SeedChanged(seed) => {
                if let Some(setup) = self.training_setup.as_mut() {
                    setup.seed = seed.chars().take(64).collect();
                }
                None
            }
            TrainingSetupMessage::Cancel => {
                self.training_setup = None;
                None
            }
            TrainingSetupMessage::Start => {
                let setup = self.training_setup.take()?;
                let seed = setup.seed.trim();
                Some(Effect::StartTraining(crate::session::training::TrainingOptions {
                    match_type: setup.match_type,
                    local_player_index: setup.side,
                    seed: (!seed.is_empty()).then(|| seed.to_string()),
                    script: None,
                }))
            }
        }
    }

    /// The setup modal layer, stacked over the whole tab while a draft
    /// is open. Same modal family as the disconnect confirm: framed
    /// panel, dimmed click-to-dismiss backdrop.
    pub(super) fn training_setup_modal<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        loaded: &'a selection::Loaded,
        setup: &'a TrainingSetup,
    ) -> Element<'a, Message> {
        let msg = |m: TrainingSetupMessage| Message::TrainingSetup(m);

        // Match type, same table + naming as the lobby's picker.
        let (family, _) = loaded.game.family_and_variant();
        let mt_table = game::from_gamedb_entry(loaded.game)
            .map(|g| g.match_types)
            .unwrap_or(&[]);
        let mt_options: Vec<widgets::Choice<(u8, u8)>> = mt_table
            .iter()
            .enumerate()
            .flat_map(|(mode, &subtypes)| (0..subtypes).map(move |sub| ((mode as u8), (sub as u8))))
            .map(|(mode, sub)| {
                widgets::Choice::new((mode, sub), game::match_type_name(lang, family, mode, sub))
            })
            .collect();
        let selected_mt = mt_options.iter().find(|c| c.value == setup.match_type).cloned();
        let mt_picker = widgets::picker(mt_options, selected_mt, move |c| {
            msg(TrainingSetupMessage::MatchTypeSelected(c.value))
        })
        .width(Fill);

        let side_options = vec![
            widgets::Choice::new(0u8, t!(lang, "training-side-p1")),
            widgets::Choice::new(1u8, t!(lang, "training-side-p2")),
        ];
        let selected_side = side_options.iter().find(|c| c.value == setup.side).cloned();
        let side_picker = widgets::picker(side_options, selected_side, move |c| {
            msg(TrainingSetupMessage::SideSelected(c.value))
        })
        .width(Fill);

        let seed_input = text_input(&t!(lang, "training-seed-placeholder"), &setup.seed)
            .on_input(move |s| msg(TrainingSetupMessage::SeedChanged(s)))
            .padding([6.0, 10.0])
            .style(widgets::chunky_text_input);

        let buttons = row![
            Space::new().width(Fill),
            button(text(t!(lang, "training-cancel")).size(TEXT_BODY))
                .padding(STANDARD_PADDING)
                .style(widgets::neutral)
                .on_press(msg(TrainingSetupMessage::Cancel)),
            button(text(t!(lang, "training-start")).size(TEXT_BODY))
                .padding(STANDARD_PADDING)
                .style(widgets::primary_button)
                .on_press(msg(TrainingSetupMessage::Start)),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let panel = container(
            column![
                text(t!(lang, "training-setup-title")).size(TEXT_TITLE),
                widgets::option_row(t!(lang, "lobby-match-type"), mt_picker),
                widgets::option_row(t!(lang, "training-side"), side_picker),
                widgets::option_row(t!(lang, "training-seed"), seed_input),
                buttons,
            ]
            .spacing(10),
        )
        .padding(style::PANE_PADDING)
        .width(Length::Fixed(440.0))
        .style(widgets::panel);

        widgets::modal_layer(
            panel.into(),
            0.55,
            Message::Noop,
            Some(msg(TrainingSetupMessage::Cancel)),
        )
    }
}
