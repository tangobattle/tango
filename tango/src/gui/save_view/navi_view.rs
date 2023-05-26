use crate::gui;
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

pub fn show<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    navi_view: &tango_dataview::save::NaviView<'a>,
    assets: &(dyn tango_dataview::rom::Assets + Send + Sync),
    state: &mut State,
    prefer_vertical: bool,
) {
    match navi_view {
        tango_dataview::save::NaviView::LinkNavi(link_navi_view) => link_navi_view::show(
            ui,
            clipboard,
            font_families,
            lang,
            game_lang,
            link_navi_view.as_ref(),
            assets,
            &mut state.link_navi_view,
        ),
        tango_dataview::save::NaviView::Navicust(navicust_view) => navicust_view::show(
            ui,
            clipboard,
            font_families,
            lang,
            game_lang,
            navicust_view.as_ref(),
            assets,
            &mut state.navicust_view,
            prefer_vertical,
        ),
    }
}
