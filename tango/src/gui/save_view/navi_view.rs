use crate::{config, gui};
mod link_navi_view;
mod navicust_view;

pub struct State {
    link_navi_view: link_navi_view::State,
    navicust_view: navicust_view::State,
}

impl State {
    pub fn new() -> Self {
        Self {
            link_navi_view: link_navi_view::State::new(),
            navicust_view: navicust_view::State::new(),
        }
    }
}

pub fn show(
    ui: &mut egui::Ui,
    config: &config::Config,
    shared_root_state: &mut gui::SharedRootState,
    game_lang: &unic_langid::LanguageIdentifier,
    navi_view: &tango_dataview::save::NaviView,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    state: &mut State,
    prefer_vertical: bool,
) {
    match navi_view {
        tango_dataview::save::NaviView::LinkNavi(link_navi_view) => link_navi_view::show(
            ui,
            config,
            shared_root_state,
            game_lang,
            link_navi_view.as_ref(),
            assets,
            &mut state.link_navi_view,
        ),
        tango_dataview::save::NaviView::Navicust(navicust_view) => navicust_view::show(
            ui,
            config,
            shared_root_state,
            game_lang,
            navicust_view.as_ref(),
            assets,
            &mut state.navicust_view,
            prefer_vertical,
        ),
    }
}
