//! The per-game save-editor interface.
//!
//! Each game's UI crate (in the `gamesupport` submodule) implements
//! [`SaveUi`] and hands tango a `&'static` instance; the shell in
//! [`crate::save_view`] dispatches every game-shaped question — which
//! section tabs exist, how a tab's body renders, what its clipboard
//! forms are — through it instead of probing the dataview generically.
//! The shared chrome (tab strip, navi header, edit-session state
//! machine, Save/Cancel) stays in the shell; game knowledge lives on
//! the other side of this trait.

use crate::loaded::OpenSave;
use crate::save_view::{RenderOpts, State, Tab};
use iced::Element;
use unic_langid::LanguageIdentifier;

pub use crate::save_view::Action;

pub trait SaveUi: Send + Sync {
    /// The section tabs this game's save editor offers, in display
    /// order. The shell prepends [`Tab::Cover`] itself in streamer mode.
    /// Called per frame — must stay cheap. Capability that genuinely
    /// varies at runtime (a BN6 link navi has no navicust) is this
    /// method's to probe; everything else should be declared statically.
    fn tabs(&self, loaded: &OpenSave) -> Vec<Tab>;

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

    /// Whether the edit session may commit right now. Required (no
    /// default): the Battle Network folder rule lives in the private UI
    /// layer as `bn_folder_can_save`, which the BN games' impls call;
    /// BCC's program deck legally has gaps and answers `true`.
    fn can_save(&self, loaded: &OpenSave) -> bool;
}
