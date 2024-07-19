use crate::config;

slotmap::new_key_type! {
    pub struct UiWindowKey;
}

type ShowCallback = Box<dyn FnMut(UiWindowKey, &egui::Context, &mut super::State, &mut config::Config) -> bool>;

#[derive(Default)]
pub struct UiWindows {
    windows: slotmap::SlotMap<UiWindowKey, ShowCallback>,
}

impl UiWindows {
    pub fn push(
        &mut self,
        show: impl FnMut(UiWindowKey, &egui::Context, &mut super::State, &mut config::Config) -> bool + 'static,
    ) {
        self.windows.insert(Box::new(show));
    }

    pub fn show(&mut self, ctx: &egui::Context, state: &mut super::State, config: &mut config::Config) {
        self.windows.retain(|key, show| (show)(key, ctx, state, config));
    }

    pub fn merge(&mut self, ui_windows: UiWindows) {
        for (_, show) in ui_windows.windows {
            self.windows.insert(show);
        }
    }
}
