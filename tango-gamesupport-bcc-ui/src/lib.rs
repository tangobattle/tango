//! Battle Chip Challenge's save-editor UI.
//!
//! BCC's deck isn't a Battle Network folder — it's the PET's program
//! deck: a wired circuit board of eleven chip slots the battle engine
//! walks in position order, with two manually-triggered side slots on
//! the R and L buttons. The [`deck`] module draws it that way, laid out
//! like the game's own PRG DECK screen instead of a flat chip list.

use tango_gamesupport_common_ui::editor::loaded::OpenSave;
use tango_gamesupport_common_ui::editor::view as sv;
use tango_gamesupport_common_ui::editor::view::{Action, RenderOpts, State, Tab};
use tango_gamesupport_common_ui::editor::{BuildReport, GameSaveEditor, SaveEditorShell};
use unic_langid::LanguageIdentifier;

pub mod deck;

pub struct Ui;

/// The instance tango's per-family registry hands out.
pub static SAVE_EDITOR: SaveEditorShell<Ui> = SaveEditorShell(Ui);

impl GameSaveEditor for Ui {
    fn tabs(&self, _loaded: &OpenSave) -> Vec<Tab> {
        vec![Tab::ProgramDeck]
    }

    fn render<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        _opts: RenderOpts,
    ) -> iced::Element<'a, Action> {
        match tab {
            Tab::Cover => sv::cover::render_cover(lang, loaded),
            Tab::ProgramDeck => deck::render(lang, loaded),
            _ => sv::placeholder(tango_gamesupport_common_ui::t!(lang, "save-empty")),
        }
    }

    fn render_edit<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        state: &'a State,
    ) -> iced::Element<'a, Action> {
        match tab {
            Tab::ProgramDeck => deck::render_edit(lang, loaded, state),
            _ => sv::placeholder(tango_gamesupport_common_ui::t!(lang, "save-empty")),
        }
    }

    fn tab_as_text(&self, _lang: &LanguageIdentifier, tab: Tab, loaded: &OpenSave, _opts: RenderOpts) -> Option<String> {
        match tab {
            Tab::ProgramDeck => deck::as_text(loaded),
            _ => None,
        }
    }

    /// A program deck may legally have empty slots, but its Navi socket and
    /// MB budgets still participate in the shared legality report.
    fn build_report(&self, loaded: &OpenSave) -> BuildReport {
        deck::build_report(loaded)
    }
}
