use crate::config;

slotmap::new_key_type! {
    pub struct UiWindowKey;
}

type ShowCallback =
    Box<dyn FnMut(UiWindowKey, &egui::Context, &mut config::Config, &mut super::SharedRootState) -> bool>;

#[derive(Default)]
pub struct UiWindows {
    windows: slotmap::SlotMap<UiWindowKey, ShowCallback>,
}

impl UiWindows {
    pub fn push(
        &mut self,
        show: impl FnMut(UiWindowKey, &egui::Context, &mut config::Config, &mut super::SharedRootState) -> bool + 'static,
    ) {
        self.windows.insert(Box::new(show));
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        config: &mut config::Config,
        shared_root_state: &mut super::SharedRootState,
    ) {
        self.windows
            .retain(|key, show| (show)(key, ctx, config, shared_root_state));
    }

    pub fn merge(&mut self, ui_windows: UiWindows) {
        for (_, show) in ui_windows.windows {
            self.windows.insert(show);
        }
    }
}
