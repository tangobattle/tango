//! The save editor — everything that loads, draws, and edits a save.
//!
//! [`GameSaveEditor`] is the per-game interface: each
//! `tango-gamesupport-GAME-ui` crate implements it and wraps it in a
//! [`SaveEditorShell`], the one implementation of the public
//! `tango_gamesupport::SaveEditor` embedding API (in [`shell`], along
//! with the opaque-envelope plumbing). [`view`] holds the shell view +
//! shared components the implementations compose; [`loaded`] bakes the
//! model into the render-ready [`loaded::OpenSave`] the whole editor
//! reads from.

pub mod loaded;
pub(crate) mod shell;
pub mod view;

pub use shell::SaveEditorShell;

use crate::editor::loaded::OpenSave;
use crate::editor::view::{RenderOpts, State, Tab};
use iced::Element;
use unic_langid::LanguageIdentifier;

pub use crate::editor::view::Action;

pub trait GameSaveEditor: Send + Sync {
    /// The section tabs this game's save editor offers, in display
    /// order. The shell prepends [`Tab::Cover`] itself in streamer mode.
    /// Called per frame — must stay cheap. Capability that genuinely
    /// varies at runtime (a BN6 link navi has no navicust) is this
    /// method's to probe; everything else should be declared statically.
    fn tabs(&self, loaded: &OpenSave) -> Vec<Tab>;

    /// A control this game puts in the editor's top bar, left of Edit /
    /// Play: something that selects which save the whole editor below
    /// is showing, which is bigger than any one section. BN5DS's
    /// cartridge holds two in-game files, so it offers a file switcher
    /// there and everything under the bar is the ordinary viewer for
    /// whichever file is picked.
    ///
    /// The shell hides it while an edit session is open — changing what
    /// is being edited midway would splice staged edits across saves.
    /// Called per frame; `None` (the default) leaves the bar as it was.
    fn top_bar_control<'a>(&self, lang: &'a LanguageIdentifier, loaded: &'a OpenSave) -> Option<Element<'a, Action>> {
        let _ = (lang, loaded);
        None
    }

    /// Read-only body for `tab`. May borrow from `loaded` (most
    /// components return owned `'static` trees, which coerce).
    fn render<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        opts: RenderOpts,
    ) -> Element<'a, Action>;

    /// Editor body for `tab`. Only called while the global edit session
    /// is open *and* the tab's section is editable on this save (per
    /// [`crate::model::Editability`]).
    fn render_edit<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        state: &'a State,
    ) -> Element<'a, Action>;

    /// Whether `tab`'s section participates in the global edit session
    /// on this save. The default maps the shared
    /// [`crate::model::Editability`] capability probe; a game whose
    /// section lives outside the shared model (BN4's Mod Cards)
    /// overrides the answer for that tab.
    fn tab_editable(&self, tab: Tab, loaded: &OpenSave) -> bool {
        tab.editable_on(&loaded.editability)
    }

    /// The tab as TSV-ish clipboard text, or `None` for tabs without a
    /// text form.
    fn tab_as_text(&self, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String>;

    /// The tab as a raster image for the clipboard, or `None` for tabs
    /// without an image form.
    fn tab_as_image(&self, tab: Tab, loaded: &OpenSave) -> Option<image::RgbaImage> {
        let _ = (tab, loaded);
        None
    }

    /// Whether the edit session may commit right now. The default is the
    /// Battle Network rule: a chip folder must be completely full and
    /// inside its class/memory limits before it can be written back.
    /// Games whose folder may legally have gaps (BCC's program deck)
    /// override this.
    fn can_save(&self, loaded: &OpenSave) -> bool {
        let full = loaded.save.view_chips().is_none_or(|v| {
            let folder = v.equipped_folder_index();
            (0..v.folder_size()).all(|i| v.chip(folder, i).is_some())
        });
        full && crate::model::rules::folder_limits_satisfied(loaded)
    }
}

/// The editor of a game that models nothing behind its save (netplay
/// only): no section tabs, nothing editable. The shell renders its
/// empty state — which still carries the navi strip and its Play
/// button — so such a game reaches a session through the same editor
/// path as every other one instead of needing an editor-less one.
pub struct EmptyEditor;

/// What a netplay-only game's `Game::save_editor` points at.
pub static EMPTY_SAVE_EDITOR: SaveEditorShell<EmptyEditor> = SaveEditorShell(EmptyEditor);

impl GameSaveEditor for EmptyEditor {
    fn tabs(&self, _loaded: &OpenSave) -> Vec<Tab> {
        vec![]
    }

    // With no tabs the shell never asks for a body; the placeholder is
    // what any game answers for a tab it doesn't render.
    fn render<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        _tab: Tab,
        _loaded: &'a OpenSave,
        _opts: RenderOpts,
    ) -> Element<'a, Action> {
        crate::editor::view::placeholder(crate::t!(lang, "save-empty"))
    }

    fn render_edit<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        _tab: Tab,
        _loaded: &'a OpenSave,
        _state: &'a State,
    ) -> Element<'a, Action> {
        crate::editor::view::placeholder(crate::t!(lang, "save-empty"))
    }

    fn tab_as_text(&self, _tab: Tab, _loaded: &OpenSave, _opts: RenderOpts) -> Option<String> {
        None
    }
}
