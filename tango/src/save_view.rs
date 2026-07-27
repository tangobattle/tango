//! The save view lives in the private `tango-gamesupport-common` crate;
//! each game carries its own editor as a feature-gated `ui` module, and
//! `Game::save_ui` points at it — so there's no per-family registry
//! here, just the app-side fallback for a game compiled in without its
//! UI feature.

pub use tango_gamesupport_common::save_ui::SaveUi;
pub use tango_gamesupport_common::save_view::*;

use tango_gamesupport_common::loaded::OpenSave;
use unic_langid::LanguageIdentifier;

/// The save-editor UI for a game whose crate was built without its `ui`
/// feature (never expected in a shipped build — every `gamesupport-*`
/// feature turns it on). Deliberately game-blind: no section tabs at
/// all, so the shell shows the navi strip (Play stays reachable) over
/// the empty-save placeholder, and nothing here reaches into the
/// gamesupport components.
pub struct FallbackSaveUi;

pub static FALLBACK_SAVE_UI: FallbackSaveUi = FallbackSaveUi;

impl SaveUi for FallbackSaveUi {
    fn tabs(&self, _loaded: &OpenSave) -> Vec<Tab> {
        vec![]
    }

    fn render<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        _tab: Tab,
        _loaded: &'a OpenSave,
        _opts: RenderOpts,
    ) -> iced::Element<'a, Action> {
        placeholder(crate::i18n::t(lang, "save-empty"))
    }

    fn render_edit<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        _tab: Tab,
        _loaded: &'a OpenSave,
        _state: &'a State,
    ) -> iced::Element<'a, Action> {
        placeholder(crate::i18n::t(lang, "save-empty"))
    }

    fn tab_as_text(&self, _tab: Tab, _loaded: &OpenSave, _opts: RenderOpts) -> Option<String> {
        None
    }
}
