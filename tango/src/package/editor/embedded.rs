//! App adapter for package editors. Native launch/telemetry data remains in
//! LoadedSave during migration; this adapter owns all editor state and bytes.
use std::sync::{Arc, Mutex};

use crate::i18n::t;
use iced::widget::{column, text};
use iced::{Element, Task, Theme};
use tango_gamesupport::{LoadedSave, SaveEditor, SaveEditorEvent, SaveEditorMessage, SaveEditorState};
use tango_script::{EditorSession, Embedding, HostAction, Profile, SaveRequest};
use unic_langid::LanguageIdentifier;

type Message = Arc<dyn SaveEditorMessage>;
pub(crate) fn select<'a>(
    profiles: impl IntoIterator<Item = &'a Profile>,
    rom: &[u8],
) -> tango_script::Result<Option<Profile>> {
    let mut selected = None;
    for profile in profiles {
        if profile.supports_editor(rom)? {
            if selected.is_some() {
                return Err(tango_script::Error::Invalid(
                    "multiple package editors recognize this ROM".into(),
                ));
            }
            selected = Some(profile.clone());
        }
    }
    Ok(selected)
}

/// Package sessions can present their committed SRAM directly. A gamemode may
/// have no editor; that leaves the setup pane absent without using native data.
pub fn open_sram(
    native_game: Option<crate::library::rom::GameRef>,
    rom: &[u8],
    sram: &[u8],
    catalog: &tango_library::package::Catalog,
    selected: Option<&tango_library::package::ExportRef>,
) -> tango_script::Result<Option<LoadedSave>> {
    let Some(profile) = super::selection::resolve(catalog, rom, selected).map_err(tango_script::Error::Invalid)? else {
        return Ok(None);
    };
    open_profile(native_game, profile, rom, sram).map(Some)
}

pub(crate) fn open_profile(
    native_game: Option<crate::library::rom::GameRef>,
    profile: Profile,
    rom: &[u8],
    sram: &[u8],
) -> tango_script::Result<LoadedSave> {
    let profile = profile.with_inputs(tango_script::Inputs::new([("rom".into(), rom.to_vec())].into())?);
    Ok(LoadedSave {
        editor: &EDITOR,
        native_game,
        chips: Vec::new(),
        save_path: Default::default(),
        patch: None,
        state: Box::new(State::open(profile, sram)?),
        payload: Box::new(Payload),
    })
}

/// Replace a loaded editor when exactly one installed or bundled editor recognizes its ROM.
/// Legacy patches still depend on native ROM overrides; keep their editor until
/// that metadata is supplied by the replacement package path.
pub fn attach(data: &mut LoadedSave, rom: &[u8], sram: &[u8], catalog: &tango_library::package::Catalog) {
    if data.patch.is_some() {
        return;
    }
    let open = || -> anyhow::Result<Option<State>> {
        let Some(profile) = select(
            catalog
                .latest_exports(tango_script::ExportKind::Editor)
                .into_iter()
                .map(|export| &export.profile),
            rom,
        )?
        else {
            return Ok(None);
        };
        let profile = profile.with_inputs(tango_script::Inputs::new([("rom".to_owned(), rom.to_vec())].into())?);
        Ok(Some(State::open(profile, sram)?))
    };
    match open() {
        Ok(Some(state)) => {
            data.editor = &EDITOR;
            data.state = Box::new(state);
            // No second native save model can diverge from the package's draft.
            data.payload = Box::new(Payload);
        }
        Ok(None) => {}
        Err(error) => log::error!("package editor unavailable; retaining native editor: {error:#}"),
    }
}

struct Payload;
impl tango_gamesupport::LoadedSavePayload for Payload {}

#[derive(Clone, PartialEq, Eq)]
struct Configuration {
    locale: String,
    editable: bool,
    embedding: Embedding,
}

struct Controller {
    session: EditorSession,
    configuration: Option<Configuration>,
    blocked: bool,
    epoch: u64,
    pending: Option<SaveRequest>,
    error: Option<anyhow::Error>,
}

impl Controller {
    fn configure(&mut self, configuration: Configuration) {
        if self.configuration.as_ref() == Some(&configuration) {
            return;
        }
        let locale_only = self.configuration.as_ref().is_some_and(|previous| {
            previous.editable == configuration.editable && previous.embedding == configuration.embedding
        });
        self.epoch = self.epoch.wrapping_add(1);
        let result = self.session.configure(
            &configuration.locale,
            configuration.editable,
            configuration.embedding.clone(),
        );
        let blocked = result.is_err();
        if blocked || self.blocked || !locale_only {
            self.error = result.err().map(Into::into);
        }
        self.blocked = blocked;
        // Remember failed configurations too, so redraws don't rerun a broken
        // callback every frame. Another configuration change can retry it.
        self.configuration = Some(configuration);
    }
}

struct State {
    controller: Mutex<Controller>,
    renderer: tango_script_iced::Renderer,
    identity: Arc<()>,
}
impl SaveEditorState for State {}

impl State {
    fn open(profile: Profile, sram: &[u8]) -> tango_script::Result<Self> {
        Ok(Self {
            controller: Mutex::new(Controller {
                session: EditorSession::open(profile, sram, false)?,
                configuration: None,
                blocked: false,
                epoch: 0,
                pending: None,
                error: None,
            }),
            renderer: Default::default(),
            identity: Arc::new(()),
        })
    }
}

fn state(data: &LoadedSave) -> &State {
    (data.state.as_ref() as &dyn std::any::Any)
        .downcast_ref()
        .expect("package editor state")
}

#[derive(Debug)]
struct Input {
    identity: Arc<()>,
    epoch: u64,
    action: tango_script_iced::Message,
}
impl SaveEditorMessage for Input {}

struct Editor;
static EDITOR: Editor = Editor;

#[cfg(test)]
mod tests;

impl SaveEditor for Editor {
    fn view<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        data: &'a LoadedSave,
        streamer_mode: bool,
        play_button: Option<bool>,
        inline_actions: bool,
        editable: bool,
    ) -> Element<'a, Message> {
        let state = state(data);
        let mut controller = state.controller.lock().unwrap();
        controller.configure(Configuration {
            locale: lang.to_string(),
            editable: editable && !data.save_path.as_os_str().is_empty(),
            embedding: Embedding {
                streamer_mode,
                inline_actions,
                actions: play_button
                    .map(|enabled| {
                        (
                            "play".into(),
                            HostAction {
                                enabled,
                                while_editing: false,
                            },
                        )
                    })
                    .into_iter()
                    .collect(),
            },
        });
        let mut content = column![].spacing(8);
        if let Some(error) = &controller.error {
            let error = crate::package::error::localized(error, lang);
            content = content
                .push(text(t!(lang, "package-editor-error", error = error.as_str())).style(iced::widget::text::danger));
        }
        if !controller.blocked {
            let identity = state.identity.clone();
            let epoch = controller.epoch;
            content = content.push(
                state
                    .renderer
                    .view(controller.session.document().view())
                    .map(move |action| {
                        Arc::new(Input {
                            identity: identity.clone(),
                            epoch,
                            action,
                        }) as Message
                    }),
            );
        }
        content.width(iced::Fill).height(iced::Fill).into()
    }

    fn update(
        &self,
        lang: &LanguageIdentifier,
        data: &mut LoadedSave,
        msg: &dyn SaveEditorMessage,
        theme: &Theme,
    ) -> (Task<Message>, Vec<SaveEditorEvent>) {
        let Some(input) = (msg as &dyn std::any::Any).downcast_ref::<Input>() else {
            return (Task::none(), Vec::new());
        };
        let state = state(data);
        let mut controller = state.controller.lock().unwrap();
        if controller.blocked
            || !Arc::ptr_eq(&input.identity, &state.identity)
            || input.epoch != controller.epoch
            || controller.session.document().locale() != lang
        {
            return (Task::none(), Vec::new());
        }
        let Some(action) = &input.action else {
            return (Task::none(), Vec::new());
        };
        let update = match controller.session.dispatch(action.clone()) {
            Ok(update) => update,
            Err(error) => {
                controller.error = Some(error.into());
                return (Task::none(), Vec::new());
            }
        };
        controller.error = None;
        let mut tasks = Vec::new();
        let mut events = Vec::new();
        let mut copied = false;
        for effect in update.effects {
            match effect {
                tango_script::Effect::ScrollTo { id, x, y } => tasks.push(state.renderer.scroll_to(&id, x, y)),
                tango_script::Effect::Invoke { id } => match id.as_str() {
                    "play" => events.push(SaveEditorEvent::Play),
                    _ => {
                        controller.error = Some(anyhow::Error::msg(t!(
                            lang,
                            "package-editor-unknown-action",
                            id = id.as_str()
                        )))
                    }
                },
                tango_script::Effect::Edit { .. } => unreachable!("session consumes edit commands"),
                effect => {
                    copied = true;
                    if let Err(error) = crate::package::editor::clipboard::write(&state.renderer, theme, effect) {
                        controller.error = Some(error);
                    }
                }
            }
        }
        if copied && controller.error.is_none() {
            state.renderer.acknowledge(&action.id);
        }
        if let Some(request) = update.save {
            events.push(SaveEditorEvent::Commit {
                sram: request.bytes().to_vec(),
            });
            controller.pending = Some(request);
        }
        (Task::batch(tasks), events)
    }

    fn sram(&self, data: &LoadedSave) -> Result<Vec<u8>, String> {
        let controller = state(data).controller.lock().unwrap();
        if controller.blocked {
            return Err(controller
                .error
                .as_ref()
                .map(|error| crate::package::error::localized(error, controller.session.document().locale()))
                .unwrap_or_else(|| t!(controller.session.document().locale(), "package-editor-not-configured")));
        }
        controller
            .session
            .document()
            .snapshot()
            .map_err(|error| error.localized(controller.session.document().locale()))
    }

    fn save_finished(&self, data: &mut LoadedSave, result: Result<(), String>) {
        let mut controller = state(data).controller.lock().unwrap();
        if let Some(request) = controller.pending.take() {
            let finish = controller.session.finish_save(&request, result.is_ok());
            if let Some(error) = result
                .err()
                .map(anyhow::Error::msg)
                .or_else(|| finish.err().map(Into::into))
            {
                controller.error = Some(error);
            }
        }
    }

    fn restart_view_entrance(&self, state: &mut dyn SaveEditorState) {
        if let Some(state) = (state as &dyn std::any::Any).downcast_ref::<State>() {
            state.renderer.restart_motion();
        }
    }
}
