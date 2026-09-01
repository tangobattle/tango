//! The adapter between the public `tango_gamesupport::SaveEditor`
//! embedding API and this crate's [`GameSaveEditor`] implementations:
//! the generic [`SaveEditorShell`], the opaque-envelope plumbing (the
//! marker-trait impls for `Action` / `State` / `OpenSave` and the
//! upcast helpers that reopen them — the only downcasts anywhere), and
//! the free functions the app drives game-independent state with.

use crate::editor::loaded::{self, OpenSave};
use crate::editor::view::{Action, Outcome, State};
use crate::editor::GameSaveEditor;
use tango_gamesupport::LoadedSave;
use unic_langid::LanguageIdentifier;

/// What every game crate's `Game::save_editor` slot points at: its
/// [`GameSaveEditor`] wrapped in the one implementation of the public
/// `tango_gamesupport::SaveEditor`. The public trait speaks opaque marker
/// traits ([`SaveEditorMessage`](tango_gamesupport::SaveEditorMessage) et al.);
/// this crate's [`Action`], [`State`] and [`OpenSave`] are the types
/// behind them, recovered by the upcast helpers below.
pub struct SaveEditorShell<G>(pub G);

/// Serialize a save for a session without mutating the editor's staged copy.
///
/// Edits are applied to that copy as they happen, while its checksum is only
/// rebuilt when the user saves. A match can start before then, so session
/// snapshots must repair a clone rather than serializing the checksum-stale
/// editor value (or implicitly committing it to disk).
fn session_sram(save: &(dyn crate::dataview::save::Save + Send + Sync)) -> Vec<u8> {
    let mut snapshot = save.clone_box();
    snapshot.rebuild_checksum();
    snapshot.to_sram_dump()
}

impl tango_gamesupport::SaveEditorMessage for Action {}
impl tango_gamesupport::SaveEditorState for State {}
impl tango_gamesupport::LoadedSavePayload for OpenSave {}

fn view_state(state: &dyn tango_gamesupport::SaveEditorState) -> &State {
    (state as &dyn std::any::Any)
        .downcast_ref::<State>()
        .expect("SaveEditorState must be this crate's State")
}

fn view_state_mut(state: &mut dyn tango_gamesupport::SaveEditorState) -> &mut State {
    (state as &mut dyn std::any::Any)
        .downcast_mut::<State>()
        .expect("SaveEditorState must be this crate's State")
}

fn wrap(action: Action) -> std::sync::Arc<dyn tango_gamesupport::SaveEditorMessage> {
    std::sync::Arc::new(action)
}

#[derive(Debug)]
struct Warnings(Vec<tango_gamesupport::OpaqueBuildWarnings>);

impl tango_gamesupport::BuildWarnings for Warnings {
    fn format(&self, lang: &LanguageIdentifier) -> Vec<String> {
        self.0
            .iter()
            .flat_map(|warnings| warnings.format(lang))
            .collect()
    }
}

impl<G: GameSaveEditor + 'static> tango_gamesupport::SaveEditor for SaveEditorShell<G> {
    fn validate_save(
        &self,
        prepared: &tango_gamesupport::PreparedSave,
    ) -> Option<tango_gamesupport::OpaqueBuildWarnings> {
        let save = crate::dataview::save_ref(prepared.save.as_ref());
        let assets = crate::dataview::assets_ref(prepared.assets.as_ref());
        let warnings = self.0.validate_save(save, assets);
        (!warnings.is_empty()).then(|| {
            std::sync::Arc::new(Warnings(warnings)) as tango_gamesupport::OpaqueBuildWarnings
        })
    }

    fn load(
        &'static self,
        prepared: tango_gamesupport::PreparedSave,
    ) -> LoadedSave {
        let model = crate::model::from_prepared(prepared);
        let game = model.game;
        let save_path = model.save_path.clone();
        let patch = model.patch.clone();
        let open = crate::editor::loaded::from_model(model, &self.0);
        LoadedSave {
            editor: self,
            game,
            chips: crate::editor::loaded::chip_display_table(&open),
            save_path,
            patch,
            state: Box::new(State::new()),
            payload: Box::new(open),
        }
    }

    fn view<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        data: &'a LoadedSave,
        streamer_mode: bool,
        play_button: Option<bool>,
        inline_actions: bool,
        editable: bool,
    ) -> iced::Element<'a, std::sync::Arc<dyn tango_gamesupport::SaveEditorMessage>> {
        crate::editor::view::view(
            lang,
            loaded::open(data),
            view_state(&*data.state),
            streamer_mode,
            play_button,
            inline_actions,
            editable,
        )
        .map(wrap)
    }

    fn update(
        &self,
        lang: &LanguageIdentifier,
        data: &mut LoadedSave,
        msg: &dyn tango_gamesupport::SaveEditorMessage,
    ) -> (
        iced::Task<std::sync::Arc<dyn tango_gamesupport::SaveEditorMessage>>,
        Option<tango_gamesupport::SaveEditorEvent>,
    ) {
        use tango_gamesupport::SaveEditorEvent as Out;

        let Some(action) = (msg as &dyn std::any::Any).downcast_ref::<Action>() else {
            return (iced::Task::none(), None);
        };
        // Disjoint fields of the same save: the view state folds the
        // action, the bundle behind the payload backs it.
        let state = view_state_mut(&mut *data.state);
        let open = loaded::open_mut(&mut *data.payload);

        let (task, outcome) = state.apply(lang, action, Some(&*open));
        let outcome = match outcome {
            // Staged edits land in the loaded bundle right here — the
            // app never sees them, it just keeps rendering.
            Some(Outcome::Edit(edit)) => {
                let invalidated = crate::model::apply_edit(&mut open.model, edit);
                if invalidated.navicust_render {
                    crate::editor::loaded::rebuild_navicust_render(open);
                }
                None
            }
            Some(Outcome::Commit) => {
                // Every staged edit already kept its derived caches in
                // sync; commit recomputes the whole-SRAM checksum and
                // hands the app the bytes to write. Re-bake the
                // navi-view image too — commit keeps this in-memory
                // bundle, so without it the read-only grid would lag
                // until reselection.
                open.save.rebuild_checksum();
                crate::editor::loaded::rebuild_navicust_render(open);
                Some(Out::Commit {
                    sram: open.save.to_sram_dump(),
                })
            }
            Some(Outcome::Cancel) => Some(Out::Cancel),
            Some(Outcome::CopyText(s)) => Some(Out::CopyText(s)),
            Some(Outcome::CopyHtml { text, html }) => Some(Out::CopyHtml { text, html }),
            Some(Outcome::CopyImage(img)) => Some(Out::CopyImage(img)),
            Some(Outcome::Play) => Some(Out::Play),
            Some(Outcome::Training) => Some(Out::Training),
            None => None,
        };
        (task.map(wrap), outcome)
    }

    fn carry_view_position(
        &self,
        from: &dyn tango_gamesupport::SaveEditorState,
        into: &mut dyn tango_gamesupport::SaveEditorState,
    ) {
        let from = view_state(from);
        view_state_mut(into).carry_position_from(from);
    }

    /// What a session started from here runs on — netplay's committed
    /// save and training's alike. The file's own bytes, whole: a file
    /// holding several saves says which one is played in those bytes
    /// (BN5DS stamps the picked file as the cartridge's most recently
    /// saved one), never beside them. Rebuild the checksum on a clone:
    /// staged edits deliberately leave the editor's checksum stale until
    /// Save, but starting a match must still produce valid SRAM without
    /// committing those edits to disk.
    fn sram(&self, data: &LoadedSave) -> Vec<u8> {
        session_sram(loaded::open(data).save.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::session_sram;
    use crate::dataview::save::Save as _;
    use std::borrow::Cow;

    #[derive(Clone)]
    struct TestSave {
        value: u8,
        checksum: u8,
    }

    impl crate::dataview::save::Save for TestSave {
        fn to_sram_dump(&self) -> Vec<u8> {
            vec![self.value, self.checksum]
        }

        fn as_raw_wram(&self) -> Cow<'_, [u8]> {
            Cow::Owned(self.to_sram_dump())
        }

        fn rebuild_checksum(&mut self) {
            self.checksum = self.value;
        }
    }

    #[test]
    fn session_sram_repairs_a_clone_without_committing_the_editor_copy() {
        let staged = TestSave {
            value: 0x42,
            checksum: 0x11,
        };

        assert_eq!(session_sram(&staged), vec![0x42, 0x42]);
        assert_eq!(staged.to_sram_dump(), vec![0x42, 0x11]);
    }
}
