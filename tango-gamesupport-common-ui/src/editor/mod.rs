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
pub use tango_gamesupport::BuildViolation;

pub trait GameSaveEditor: Send + Sync {
    /// The section tabs this game's save editor offers, in display
    /// order. The shell prepends [`Tab::Cover`] itself in streamer mode.
    /// Called per frame — must stay cheap. Capability that genuinely
    /// varies at runtime (a BN6 link navi has no navicust) is this
    /// method's to probe; everything else should be declared statically.
    fn tabs(&self, loaded: &OpenSave) -> Vec<Tab>;

    /// A control this game puts in the editor's top bar, left of Save /
    /// Cancel: a whole-save choice, bigger than any one section's body.
    /// BN5DS's cartridge holds two in-game files, so it offers a file
    /// switcher there — picking one both points the editor at that file
    /// and stages the cartridge edit that makes it the played one.
    ///
    /// Rendered only **while an edit session is open**: what it changes
    /// is an edit like any other, staged now and written on Save. A
    /// read-only view (a pvp setup pane, the replay viewer, the folder
    /// viewer before Edit) never shows it — there the save is what it
    /// is. Called per frame; `None` (the default) leaves the bar as it
    /// was.
    fn top_bar_control<'a>(&self, lang: &'a LanguageIdentifier, loaded: &'a OpenSave) -> Option<Element<'a, Action>> {
        let _ = (lang, loaded);
        None
    }

    /// The strip above the tab body: what identifies the save on the
    /// left, the `actions` cluster (Edit / Play, or Save / Cancel while
    /// editing) on the right.
    ///
    /// The default is the shared navi strip — the card BN5/BN6 name
    /// their navi in, becoming the change-navi button when `edit` is
    /// `Some`. A game whose identity is not a navi off that roster
    /// overrides this and builds its own card instead, handing it to
    /// [`view::navi::render_identity_strip`] so the strip stays one
    /// strip: BN5DS names the GBA-slot cross there, and a save with no
    /// cross is plain MegaMan, which is exactly what the slot is for.
    ///
    /// `editing` is whether the global edit session is open, so an
    /// override can read as a name while the view is a viewer and become
    /// the pick itself while it isn't.
    ///
    /// [`view::navi::render_identity_strip`]: crate::editor::view::navi::render_identity_strip
    fn identity_strip<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        loaded: &'a OpenSave,
        edit: Option<Action>,
        editing: bool,
        actions: Element<'a, Action>,
    ) -> Element<'a, Action> {
        let _ = editing;
        crate::editor::view::navi::render_navi_strip(lang, loaded, edit, actions)
    }

    /// Whether this save has something to edit that the shared
    /// [`crate::model::Editability`] sections don't cover — a game
    /// whose only editable thing is its own [`top_bar_control`], say.
    /// Ors into the probe that decides whether the Edit button appears
    /// at all; the default is for a game with nothing outside the
    /// shared sections.
    ///
    /// [`top_bar_control`]: GameSaveEditor::top_bar_control
    fn extra_editable(&self, loaded: &OpenSave) -> bool {
        let _ = loaded;
        false
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
    /// text form. Section headers localize through `lang`, the same
    /// keys the rendered view uses.
    fn tab_as_text(&self, lang: &LanguageIdentifier, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String>;

    /// The tab as a raster image for the clipboard, or `None` for tabs
    /// without an image form.
    fn tab_as_image(&self, tab: Tab, loaded: &OpenSave) -> Option<image::RgbaImage> {
        let _ = (tab, loaded);
        None
    }

    /// Structured source of truth for the PvP advisory and per-tab legality
    /// indicators. Most legality errors remain saveable; an incomplete Battle
    /// Network folder is the one hard save blocker enforced by the shared view.
    fn build_violations(&self, loaded: &OpenSave) -> Vec<BuildViolation> {
        let mut violations = crate::model::rules::folder_violations(loaded);
        violations.extend(crate::model::rules::patch_card56_violations(loaded));
        violations
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

    fn tab_as_text(&self, _lang: &LanguageIdentifier, _tab: Tab, _loaded: &OpenSave, _opts: RenderOpts) -> Option<String> {
        None
    }
}
