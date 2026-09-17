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

/// What each game registers as its editor: its
/// [`GameSaveEditor`] wrapped in the one implementation of the public
/// `tango_gamesupport::SaveEditor`. The public trait speaks opaque marker
/// traits ([`SaveEditorMessage`](tango_gamesupport::SaveEditorMessage) et al.);
/// this crate's [`Action`], [`State`] and [`OpenSave`] are the types
/// behind them, recovered by the upcast helpers below.
pub struct SaveEditorShell<G>(pub G);

impl tango_gamesupport::SaveEditorMessage for Action {}
impl tango_gamesupport::SaveEditorState for State {}
impl tango_gamesupport::LoadedSavePayload for OpenSave {
    fn snapshot_sram(&self) -> Vec<u8> {
        crate::model::session_sram(self.save.as_ref())
    }
}

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
        self.0.iter().flat_map(|warnings| warnings.format(lang)).collect()
    }
}

impl<G: GameSaveEditor + 'static> tango_gamesupport::SaveEditor for SaveEditorShell<G> {
    fn build_warnings(
        &self,
        prepared: &tango_gamesupport::PreparedSave,
        validation: &dyn tango_gamesupport::Validation,
    ) -> Option<tango_gamesupport::OpaqueBuildWarnings> {
        let save = crate::dataview::save_ref(prepared.save.as_ref());
        let assets = crate::dataview::assets_ref(prepared.assets.as_ref());
        let validation = (validation as &dyn std::any::Any)
            .downcast_ref::<crate::dataview::build::Validation>()
            .expect("validation must belong to the dataview model");
        let warnings = self.0.build_warnings(save, assets, validation);
        (!warnings.is_empty())
            .then(|| std::sync::Arc::new(Warnings(warnings)) as tango_gamesupport::OpaqueBuildWarnings)
    }

    fn load(&'static self, prepared: tango_gamesupport::PreparedSave) -> LoadedSave {
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

    fn restart_view_entrance(&self, state: &mut dyn tango_gamesupport::SaveEditorState) {
        view_state_mut(state).restart_entrance();
    }
}
