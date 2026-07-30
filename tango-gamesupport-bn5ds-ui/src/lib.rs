//! BN5DS's save-editor UI: the chip folder, editable now that the
//! flash checksums are reversed, plus a switcher for the cart's two
//! in-game files in the editor's top bar. The tabs probe the views the
//! same way BN5's do, so everything under that bar is the ordinary
//! save viewer; the switcher is this game's own affordance — a dump is
//! two saves (see the dataview's `SaveSet`), and picking one hands the
//! model that file's save instead of re-aiming the one it has.

use std::sync::Arc;

use tango_gamesupport_bn5ds_dataview::save::{PlayedFile, Save, SaveSet};
use tango_gamesupport_common::dataview::save::Save as _;
use tango_gamesupport_common::editor::loaded::OpenSave;
use tango_gamesupport_common::editor::view as sv;
use tango_gamesupport_common::editor::view::{Action, RenderOpts, State, Tab};
use tango_gamesupport_common::editor::{GameSaveEditor, SaveEditorShell};
use tango_gamesupport_common::model::edit::{GameEdit, Invalidation};
use unic_langid::LanguageIdentifier;

pub struct Ui;

/// The instance tango's per-family registry hands out.
pub static SAVE_EDITOR: SaveEditorShell<Ui> = SaveEditorShell(Ui);

/// This save's file, when the loaded save is one of ours.
fn file_of(loaded: &OpenSave) -> Option<&Save> {
    loaded.save.as_ref().as_any().downcast_ref::<Save>()
}

/// Show the cart's other in-game file: re-reads the set from the dump
/// the loaded save carries — staged edits included, since they live in
/// those bytes — and hands the model that file's [`Save`].
///
/// Ships as a [`GameEdit`] because that is the one path a game-specific
/// action may reach the loaded save on. Nothing about the dump changes,
/// so committing (or not) writes the same bytes either way.
#[derive(Debug)]
struct ShowFile(u8);

impl GameEdit for ShowFile {
    fn apply(&self, model: &mut tango_gamesupport_common::model::SaveModel) -> Invalidation {
        let Some(next) = model
            .save
            .as_any()
            .downcast_ref::<Save>()
            .and_then(|save| SaveSet::parse(&save.to_sram_dump()).ok())
            .and_then(|set| set.save(self.0))
        else {
            return Invalidation::default();
        };
        model.save = Box::new(next);
        // A different file can differ in what it offers to edit, so the
        // cached capability flags are re-probed against the new save.
        tango_gamesupport_common::model::refresh_editability(model);
        Invalidation::default()
    }
}

/// One of the cart's files paired with its localized label, for the
/// switcher's dropdown — the picker renders options via `Display`,
/// which can't reach the language, so the label is resolved up front.
#[derive(Clone, PartialEq)]
struct FileChoice {
    slot: u8,
    label: String,
}

impl std::fmt::Display for FileChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The switcher the shell puts in the top bar: a dropdown over the
/// cart's file-select slots, naming them the way the game's own file
/// select does. `None` when there is only one file (a damaged dump) —
/// then there is nothing to switch and the editor is the plain viewer.
fn file_switcher<'a>(lang: &'a LanguageIdentifier, loaded: &'a OpenSave) -> Option<iced::Element<'a, Action>> {
    let save = file_of(loaded)?;
    if save.slots().len() < 2 {
        return None;
    }
    let options: Vec<FileChoice> = save
        .slots()
        .iter()
        .map(|&slot| FileChoice {
            slot,
            label: tango_gamesupport_common::t!(lang, "save-file", num = slot as usize + 1),
        })
        .collect();
    let selected = options.iter().find(|c| c.slot == save.slot()).cloned();
    Some(
        sweeten::widget::pick_list(options, selected, |c: FileChoice| {
            Action::Game(Arc::new(ShowFile(c.slot)))
        })
        .padding(tango_gamesupport_common::style::CONTROL_PADDING)
        .text_size(tango_gamesupport_common::style::TEXT_BODY)
        .style(tango_gamesupport_common::widgets::chunky_pick_list)
        .into(),
    )
}

impl GameSaveEditor for Ui {
    fn tabs(&self, loaded: &OpenSave) -> Vec<Tab> {
        let save = loaded.save.as_ref();
        let mut tabs = vec![];
        if save.view_chips().is_some() {
            tabs.push(Tab::Folder);
        }
        tabs
    }

    fn top_bar_control<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        loaded: &'a OpenSave,
    ) -> Option<iced::Element<'a, Action>> {
        file_switcher(lang, loaded)
    }

    /// Which file the switcher is on — [`PlayedFile`], the slot a
    /// netplay commit reveals beside the dump so the peers' priming
    /// and the recording play the file on screen here.
    fn session_payload(&self, loaded: &OpenSave) -> Option<tango_match::BoxedSessionPayload> {
        Some(Box::new(PlayedFile(file_of(loaded)?.slot())))
    }

    /// Open on the payload's file — the setup panes and the replay
    /// viewer land here with the file a session actually plays, which
    /// the parse alone can't know (it opens on the cart's most recent).
    fn apply_session_payload(
        &self,
        model: &mut tango_gamesupport_common::model::SaveModel,
        payload: &dyn tango_match::SessionPayload,
    ) {
        if let Some(&PlayedFile(slot)) = (payload as &dyn std::any::Any).downcast_ref::<PlayedFile>() {
            // A slot this cart doesn't hold (a damaged dump) is a
            // no-op: the view stays on the parse's own default, which
            // is what a session falls back to as well. The invalidation
            // can be dropped — the shell applies payloads before any
            // art is baked.
            let _ = ShowFile(slot).apply(model);
        }
    }

    fn render<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        opts: RenderOpts,
    ) -> iced::Element<'a, Action> {
        match tab {
            Tab::Cover => sv::cover::render_cover(lang, loaded),
            Tab::Folder => sv::folder::render_folder(lang, loaded, opts.folder_grouped),
            _ => sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty")),
        }
    }

    // No file picker here: the edit session stages against the file it
    // was opened on, and swapping the block out from under it would
    // splice staged edits across files.
    fn render_edit<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        state: &'a State,
    ) -> iced::Element<'a, Action> {
        match tab {
            Tab::Folder => sv::folder::render_folder_edit(lang, loaded, state),
            _ => sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty")),
        }
    }

    fn tab_as_text(&self, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
        match tab {
            Tab::Folder => sv::folder::as_text(loaded, opts),
            _ => None,
        }
    }
}
