use crate::game;

pub struct State {
    game: &'static (dyn game::Game + Send + Sync),
    name: String,
}

impl State {
    pub fn new(game: &'static (dyn game::Game + Send + Sync)) -> Self {
        Self {
            game,
            name: "".to_string(),
        }
    }
}

pub fn show(ui: &mut egui::Ui, show: &mut Option<State>) {}
