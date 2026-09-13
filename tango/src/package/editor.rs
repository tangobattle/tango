//! Package tools, development window and the normal app's embedded editor.
//! Game-specific controls and operations arrive as Luau-generated nodes/actions.
use std::path::PathBuf;

use crate::i18n::t;
use iced::widget::{button, column, container, text};
use iced::{Element, Length, Task};
use sweeten::widget::row;
use tango_library::storage::{write_atomic, StdStorage, Storage};
use tango_script::{Action, EditCommand, EditorSession};

pub(crate) mod bundled;
mod clipboard;
pub(crate) mod embedded;
#[cfg(test)]
mod tests;

#[derive(clap::Args, Debug, Clone, Default)]
pub struct InputOptions {
    /// Select an editor exported by the final package (automatic if there is one).
    #[arg(long, value_name = "NAME")]
    editor: Option<String>,
    /// Select a gamemode module for ROM preparation, independently of the editor.
    #[arg(long, value_name = "NAME")]
    gamemode: Option<String>,
    /// Supply immutable data under a script-visible name (repeatable).
    #[arg(long = "input", value_name = "NAME=PATH", value_parser = parse_input)]
    files: Vec<(String, PathBuf)>,
    /// Transform this ROM once, then expose the result as the "rom" input.
    #[arg(long)]
    rom: Option<PathBuf>,
    /// Active language (Tango's setting when opened in-app; otherwise en-US).
    #[arg(long, value_name = "LANGUAGE")]
    locale: Option<String>,
}

fn parse_input(value: &str) -> Result<(String, PathBuf), String> {
    let (name, path) = value.split_once('=').ok_or("expected NAME=PATH")?;
    if name.is_empty() || path.is_empty() {
        return Err("expected a nonempty input name and path".into());
    }
    Ok((name.to_owned(), path.into()))
}

async fn prepare_profile(packages: &[PathBuf], inputs: &InputOptions) -> anyhow::Result<tango_script::Profile> {
    let mut profile = tango_library::package::load_profile(&StdStorage, packages)
        .await?
        .with_locale(inputs.locale.as_deref().unwrap_or("en-US"))?;
    if let Some(editor) = &inputs.editor {
        profile = profile.with_editor(editor)?;
    }
    if let Some(mode) = &inputs.gamemode {
        profile = profile.with_gamemode(mode)?;
    }
    anyhow::ensure!(
        inputs.files.len() + usize::from(inputs.rom.is_some()) <= tango_script::MAX_INPUTS,
        "too many inputs"
    );
    let mut files = std::collections::BTreeMap::new();
    let mut remaining = tango_script::MAX_INPUT_BYTES;
    for (name, path) in &inputs.files {
        anyhow::ensure!(!files.contains_key(name), "duplicate input: {name}");
        anyhow::ensure!(
            name != "rom" || inputs.rom.is_none(),
            "use either --rom or --input rom=PATH"
        );
        let bytes = read_file(path, remaining)?;
        remaining -= bytes.len();
        files.insert(name.clone(), bytes);
    }
    if let Some(path) = &inputs.rom {
        let original = read_file(path, tango_script::MAX_BUFFER.min(remaining))?;
        let effective = profile.patch_rom(&original)?;
        anyhow::ensure!(effective.len() <= remaining, "input buffers exceed size limit");
        files.insert("rom".into(), effective);
    }
    Ok(profile.with_inputs(tango_script::Inputs::new(files)?))
}

fn read_file(path: &std::path::Path, limit: usize) -> anyhow::Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    StdStorage.open(path)?.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= limit,
        "{} exceeds the {} byte limit",
        path.display(),
        limit
    );
    Ok(bytes)
}

#[derive(clap::Subcommand, Debug, Clone)]
pub enum Command {
    /// Strictly type-check packages and resolve the selected dependency graph.
    Check {
        #[arg(required = true)]
        packages: Vec<PathBuf>,
    },
    /// Run the selected package's scripted conformance tests.
    Test {
        #[arg(required = true)]
        packages: Vec<PathBuf>,
        #[command(flatten)]
        inputs: InputOptions,
    },
    /// Create a distributable .tangopkg from a directory package.
    Bundle { source: PathBuf, output: PathBuf },
    /// Install an immutable package version into a package library.
    Install {
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        /// Defaults to the packages directory in Tango's configured data folder.
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Open the package development editor (not the embedded Tango editor).
    /// The final package is selected.
    Edit {
        #[arg(long)]
        save: PathBuf,
        /// Render the package's viewer and reject document mutations.
        #[arg(long)]
        read_only: bool,
        /// Exercise the package's streamer cover and explicit reveal flow.
        #[arg(long)]
        streamer_mode: bool,
        #[arg(required = true)]
        packages: Vec<PathBuf>,
        #[command(flatten)]
        inputs: InputOptions,
    },
    /// Apply the selected package's ROM transformations to an input file.
    PatchRom {
        #[arg(long, value_name = "NAME")]
        gamemode: Option<String>,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(required = true)]
        packages: Vec<PathBuf>,
    },
}

pub fn run_cli(command: &Command) -> Option<anyhow::Result<()>> {
    if matches!(command, Command::Edit { .. }) {
        return None;
    }
    Some(futures::executor::block_on(async {
        match command {
            Command::Check { packages } => {
                let profile = tango_library::package::load_profile(&StdStorage, packages).await?;
                let digest: String = profile.digest().iter().map(|b| format!("{b:02x}")).collect();
                println!("{}: {digest}", profile.display_name());
                for kind in tango_script::ExportKind::ALL {
                    for export in profile.packages().last().expect("resolved profile").exports(kind) {
                        println!("  {} {}: {}", kind.as_str(), export.name, export.path);
                    }
                }
            }
            Command::Test { packages, inputs } => {
                let profile = prepare_profile(packages, inputs).await?;
                profile.test()?;
                let digest: String = profile.digest().iter().map(|b| format!("{b:02x}")).collect();
                println!("{}: {digest}", profile.display_name());
            }
            Command::Bundle { source, output } => {
                tango_library::package::bundle(&StdStorage, source, output).await?;
                println!("{}", output.display());
            }
            Command::Install { sources, root } => {
                let root = root
                    .clone()
                    .unwrap_or_else(|| crate::config::Config::load_or_create().packages_path());
                let bundled = bundled::packages().map_err(anyhow::Error::msg)?;
                for package in tango_library::package::install_many(&StdStorage, sources, &root, bundled).await? {
                    println!(
                        "{}",
                        root.join(package.name)
                            .join(format!("{}.tangopkg", package.version))
                            .display()
                    );
                }
            }
            Command::PatchRom {
                gamemode,
                input,
                output,
                packages,
            } => {
                let mut profile = tango_library::package::load_profile(&StdStorage, packages).await?;
                if let Some(mode) = gamemode {
                    profile = profile.with_gamemode(mode)?;
                }
                let bytes = profile.patch_rom(&read_file(input, tango_script::MAX_BUFFER)?)?;
                write_atomic(&StdStorage, output, &bytes)?;
                println!("{}", output.display());
            }
            Command::Edit { .. } => unreachable!(),
        }
        Ok(())
    }))
}

pub fn run(command: Command, default_locale: &str) -> iced::Result {
    let Command::Edit {
        save,
        read_only,
        streamer_mode,
        packages,
        mut inputs,
    } = command
    else {
        return Ok(());
    };
    inputs.locale.get_or_insert_with(|| default_locale.to_owned());
    iced::application(
        move || Editor::open(save.clone(), packages.clone(), inputs.clone(), read_only, streamer_mode),
        Editor::update,
        Editor::view,
    )
    .title(Editor::title)
    .theme(|_: &Editor| iced::Theme::Dark)
    .subscription(|_: &Editor| {
        let close = iced::window::close_requests().map(|_| Message::Close);
        if tango_ui::anim::any_active() {
            iced::Subscription::batch([close, iced::window::frames().map(|_| Message::Ignore)])
        } else {
            close
        }
    })
    .exit_on_close_request(false)
    .window_size((860.0, 760.0))
    .font(include_bytes!("../../fonts/NotoSans-Regular.ttf").as_slice())
    .font(include_bytes!("../../fonts/NotoSansJP-Regular.otf").as_slice())
    .font(include_bytes!("../../fonts/NotoSansSC-Regular.otf").as_slice())
    .font(include_bytes!("../../fonts/NotoSansTC-Regular.otf").as_slice())
    .font(include_bytes!("../../fonts/NotoSansMono-Regular.ttf").as_slice())
    .font(lucide_icons::LUCIDE_FONT_BYTES)
    .default_font(iced::Font::with_name("Noto Sans"))
    .run()
}

struct Editor {
    lang: unic_langid::LanguageIdentifier,
    renderer: tango_script_iced::Renderer,
    path: PathBuf,
    packages: Vec<PathBuf>,
    inputs: InputOptions,
    read_only: bool,
    streamer_mode: bool,
    document: Option<EditorSession>,
    error: Option<anyhow::Error>,
    notice: Option<String>,
    closing: bool,
}

#[derive(Clone, Debug)]
enum Message {
    Ignore,
    Action(Action),
    Undo,
    Redo,
    Save,
    SaveAs,
    Reload,
    Close,
    SaveAndClose,
    DiscardAndClose,
    CancelClose,
}

impl Editor {
    fn open(path: PathBuf, packages: Vec<PathBuf>, inputs: InputOptions, read_only: bool, streamer_mode: bool) -> Self {
        let requested = inputs
            .locale
            .as_deref()
            .unwrap_or("en-US")
            .parse()
            .unwrap_or(crate::i18n::FALLBACK_LANG);
        let lang = crate::i18n::negotiate_lang(&requested);
        let result = (|| -> anyhow::Result<EditorSession> {
            let profile = futures::executor::block_on(prepare_profile(&packages, &inputs))?;
            Ok(EditorSession::open_embedded(
                profile,
                &read_file(&path, tango_script::MAX_DOCUMENT)?,
                !read_only,
                tango_script::Embedding {
                    streamer_mode,
                    ..Default::default()
                },
            )?)
        })();
        let (document, error) = match result {
            Ok(document) => (Some(document), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            lang,
            renderer: tango_script_iced::Renderer::default(),
            path,
            packages,
            inputs,
            read_only,
            streamer_mode,
            document,
            error,
            notice: None,
            closing: false,
        }
    }

    fn title(&self) -> String {
        format!(
            "{}{} — Tango",
            self.document
                .as_ref()
                .map(|s| s.document().title().to_owned())
                .unwrap_or_else(|| t!(&self.lang, "package-editor-title")),
            if self.document.as_ref().is_some_and(|s| s.document().is_dirty()) {
                " *"
            } else {
                ""
            }
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if matches!(message, Message::Ignore) {
            return Task::none();
        }
        self.error = None;
        self.notice = None;
        match message {
            Message::Close => {
                if !self.document.as_ref().is_some_and(|s| s.document().is_dirty()) {
                    return iced::exit();
                }
                self.closing = true;
                return Task::none();
            }
            Message::DiscardAndClose => return iced::exit(),
            Message::CancelClose => {
                self.closing = false;
                return Task::none();
            }
            _ => {}
        }
        if matches!(message, Message::Reload) {
            // Reloading code must not discard in-progress edits. Keep the
            // existing document if either compilation or decoding fails.
            if self.document.as_ref().is_some_and(|s| s.document().is_dirty()) {
                self.error = Some(anyhow::Error::msg(t!(&self.lang, "package-editor-reload-dirty")));
                return Task::none();
            }
            let next = Self::open(
                self.path.clone(),
                self.packages.clone(),
                self.inputs.clone(),
                self.read_only,
                self.streamer_mode,
            );
            if next.error.is_some() {
                self.error = next.error;
            } else {
                *self = next;
            }
            return Task::none();
        }
        let Some(document) = self.document.as_mut() else {
            return Task::none();
        };
        let save_and_close = matches!(message, Message::SaveAndClose);
        let copy_action = if let Message::Action(action) = &message {
            Some(action.id.clone())
        } else {
            None
        };
        let mut path = self.path.clone();
        let result = match message {
            Message::Action(action) => document.dispatch(action),
            Message::Undo => document.undo().map(|()| Default::default()),
            Message::Redo => document.redo().map(|()| Default::default()),
            Message::Save | Message::SaveAs | Message::SaveAndClose => (|| {
                if !document.is_editing() {
                    return Err(tango_script::Error::Invalid(t!(&self.lang, "package-editor-read-only")));
                }
                if matches!(message, Message::SaveAs) {
                    let Some(selected) = rfd::FileDialog::new()
                        .set_title(t!(&self.lang, "package-editor-save-as"))
                        .set_file_name(self.path.file_name().and_then(|n| n.to_str()).unwrap_or("save.sav"))
                        .save_file()
                    else {
                        return Ok(Default::default());
                    };
                    path = selected;
                }
                document.edit(EditCommand::Save)
            })(),
            Message::Reload | Message::Close | Message::DiscardAndClose | Message::CancelClose | Message::Ignore => {
                unreachable!()
            }
        };
        let mut tasks = Vec::new();
        match result {
            Err(error) => self.error = Some(error.into()),
            Ok(update) => {
                let mut copied = false;
                for effect in update.effects {
                    match effect {
                        tango_script::Effect::ScrollTo { id, x, y } => tasks.push(self.renderer.scroll_to(&id, x, y)),
                        effect => {
                            copied = true;
                            if let Err(error) = clipboard::write(&self.renderer, &iced::Theme::Dark, effect) {
                                self.error = Some(error);
                            }
                        }
                    }
                }
                if copied && self.error.is_none() {
                    if let Some(id) = copy_action {
                        self.renderer.acknowledge(&id);
                    }
                }
                if let Some(request) = update.save {
                    let written = write_atomic(&StdStorage, &path, request.bytes());
                    // Always finish the receipt, including storage failures;
                    // failure restores the staged view and permits a retry.
                    if let Err(error) = document.finish_save(&request, written.is_ok()) {
                        self.error = Some(error.into());
                    }
                    match written {
                        Ok(()) => {
                            self.path = path;
                            self.notice = Some(t!(&self.lang, "package-editor-saved"));
                            if save_and_close && self.error.is_none() {
                                return iced::exit();
                            }
                        }
                        Err(error) => self.error = Some(error.into()),
                    }
                }
            }
        }
        Task::batch(tasks)
    }

    fn view(&self) -> Element<'_, Message> {
        let document = self.document.as_ref().map(EditorSession::document);
        let toolbar = row![
            button(text(t!(&self.lang, "package-editor-undo")))
                .on_press_maybe(document.filter(|d| d.can_undo()).map(|_| Message::Undo)),
            button(text(t!(&self.lang, "package-editor-redo")))
                .on_press_maybe(document.filter(|d| d.can_redo()).map(|_| Message::Redo)),
            button(text(t!(&self.lang, "package-editor-save"))).on_press_maybe(
                document
                    .filter(|d| !d.is_read_only() && d.diagnostics().is_empty())
                    .map(|_| Message::Save)
            ),
            button(text(t!(&self.lang, "package-editor-save-as"))).on_press_maybe(
                document
                    .filter(|d| !d.is_read_only() && d.diagnostics().is_empty())
                    .map(|_| Message::SaveAs)
            ),
            button(text(t!(&self.lang, "package-editor-reload"))).on_press(Message::Reload),
        ]
        .spacing(8)
        .width(Length::Fill)
        .wrap();
        let mut body = column![
            text(self.title()).size(22),
            text(self.path.display().to_string()).size(12),
            toolbar
        ]
        .spacing(12);
        if self.closing {
            body = body.push(
                column![
                    text(t!(&self.lang, "package-editor-close-prompt")),
                    row![
                        button(text(t!(&self.lang, "package-editor-save-close"))).on_press(Message::SaveAndClose),
                        button(text(t!(&self.lang, "package-editor-discard"))).on_press(Message::DiscardAndClose),
                        button(text(t!(&self.lang, "package-editor-keep-editing"))).on_press(Message::CancelClose),
                    ]
                    .spacing(8)
                    .width(Length::Fill)
                    .wrap()
                ]
                .spacing(8),
            );
        }
        if let Some(error) = &self.error {
            let error = crate::package::error::localized(error, &self.lang);
            body = body.push(
                text(t!(&self.lang, "package-editor-error", error = error.as_str())).style(iced::widget::text::danger),
            );
        }
        if let Some(notice) = &self.notice {
            body = body.push(text(notice));
        }
        if let Some(document) = document {
            for diagnostic in document.diagnostics() {
                body = body.push(text(diagnostic).color(iced::Color::from_rgb8(255, 190, 100)));
            }
            body = body.push(
                container(
                    self.renderer
                        .view(document.view())
                        .map(|action| action.map(Message::Action).unwrap_or(Message::Ignore)),
                )
                .width(Length::Fill)
                .height(Length::Fill),
            );
        }
        container(body)
            .padding(20)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

pub(crate) mod selection;
