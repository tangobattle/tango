//! The Play tab: everything in one place. The full loadout selector
//! (family / save / patch pickers + save management, the latter in
//! [`save_manage`]) and the save viewer/editor fill the body, and a
//! netplay band rides the bottom —
//! the link-code strip + Fight CTA when idle, the full lobby
//! ([`lobby`]) once a connection attempt is in flight. The save view
//! stays on screen through it all, so what you're bringing to the
//! match is always visible (and switchable) even mid-lobby. The
//! selection state itself is App-level ([`crate::loadout::Loadout`])
//! so the lobby settings-resend sees every change made here live.

mod lobby;
mod save_manage;
mod training_setup;
pub use training_setup::{TrainingSetup, TrainingSetupMessage};

pub use save_manage::{create_new_save, creation_template, duplicate_save, rename_save, SaveAction};

use crate::app::Scanners;
use crate::i18n::t;
use crate::loadout::{self, Loadout};
use crate::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_TITLE};
use crate::widgets;
use crate::{config, game, rom, save_view, selection};
use iced::widget::{button, container, text, Space};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, row, text_input};
use unic_langid::LanguageIdentifier;

// ---------- Messages ----------

#[derive(Debug, Clone)]
pub enum Message {
    /// Loadout strip interaction. Routed by the App to the shared
    /// [`Loadout`] state — never reaches [`State::update`].
    Loadout(loadout::Message),
    SaveViewAction(save_view::Action),

    LinkCodeChanged(String),
    /// Copy plain text to the clipboard — the lobby's copy-link-code
    /// button. Carries the real code even when streamer mode masks it
    /// on screen.
    CopyText(String),
    FightPressed,
    Disconnect,
    /// Lobby UI: user picked a different match type. App routes
    /// this through netplay::Message::SetMatchType so the resend
    /// machinery picks it up.
    SetMatchType((u8, u8)),
    /// Lobby UI: user dragged the frame-delay slider, OR pressed
    /// the "suggest" button (which dispatches a value computed from the
    /// `lobby.latency_counter` median). Routes to the shared `config.frame_delay`
    /// (same store the Settings-tab slider writes), not lobby-local state.
    SetFrameDelay(u32),
    /// Lobby UI: user toggled the blind-setup checkbox.
    SetBlindSetup(bool),
    /// Lobby UI: user pressed Ready. App loads the local
    /// save's raw SRAM, builds a NegotiatedState, and
    /// dispatches netplay::Message::Commit.
    Ready,
    /// Lobby UI: user pressed Unready (Ready button while
    /// already committed). Sends an Uncommit packet.
    Unready,
    /// Soft-disable sentinel for widgets that don't accept a
    /// `None` handler in iced 0.14 (pick_list, slider). The
    /// lobby reroutes match-type / frame-delay changes here in
    /// Phase::Failed (and the selector strip during handoff) so
    /// the controls render inert without touching layout. The
    /// update handler drops it.
    Noop,

    SaveOpenFolder,
    /// Open an arbitrary folder in the OS file manager. Used by
    /// the no-saves / no-roms empty-state cards to give the user
    /// a one-click jump into the right directory.
    OpenSavesFolder(std::path::PathBuf),
    SaveDuplicateStart,
    SaveDuplicateDraftChanged(String),
    SaveDuplicateConfirm,
    SaveRenameStart,
    SaveRenameDraftChanged(String),
    SaveRenameConfirm,
    SaveDeleteStart,
    SaveDeleteConfirm,
    SaveActionCancel,
    SaveNewStart,
    SaveNewDraftChanged(String),
    /// (target variant, template name) — a template option fixes both,
    /// since the family's variants each ship their own templates.
    SaveNewTemplateSelected(rom::GameRef, String),
    SaveNewConfirm,
    /// User clicked × on the inline error banner; clears
    /// `State::last_error`.
    DismissError,

    /// Training setup modal interaction (opened by the save view's
    /// Training button; see [`training_setup`]).
    TrainingSetup(TrainingSetupMessage),
}

// ---------- Play tab state ----------

pub struct State {
    link_code: String,
    /// A link code Fight auto-generated for an empty input, parked here
    /// instead of `link_code` while the lobby is up: the outgoing strip
    /// renders for the first half of the band morph, so writing the input
    /// immediately would flash the code on screen before the lobby's
    /// connection line (which masks it in streamer mode) gets to debut it.
    /// [`restore_generated_link_code`](Self::restore_generated_link_code)
    /// moves it into the input when the lobby band leaves, so a retry
    /// re-hosts the same code.
    pending_generated_code: Option<String>,
    /// Persistent state for the embedded save view (active tab,
    /// folder grouping). Apply incoming `SaveViewAction`s via
    /// [`save_view::State::apply`].
    save_view: save_view::State,
    /// Inline state for the save-management actions (rename / delete).
    save_action: SaveAction,
    /// Last after-the-fact action failure (singleplayer launch
    /// errored, PvP session build failed, …) — rendered as a
    /// dismissable banner at the top of the tab. Pre-condition errors
    /// ("you need a save first") are NOT funneled here; they're
    /// handled by view-time button gating + inline hints, because
    /// graying out the action surface explains itself.
    last_error: Option<String>,
    /// Entrance glide for the whole save-view pane under the selector
    /// strip, played by the App when the *family* changes — a family
    /// switch replaces the entire bottom of the tab. A save switch
    /// within the family plays the smaller [`save_view::State`]
    /// entrance instead, which only moves the panes under the save
    /// view's sub-tab strip.
    save_body_enter: crate::anim::Enter,
    /// Fade-through swap for the save-action row: the picker row
    /// morphs into whichever rename / delete / create form opens
    /// ([`State::save_action`]) and back.
    save_form: crate::anim::Transition,
    /// The form that was open before `save_action` reset to
    /// `None`, frozen so the swap's exit half has something to
    /// render (the live form — including the rename draft — is
    /// already gone).
    save_action_exit: SaveAction,
    /// Training setup draft — `Some` while the modal is open.
    training_setup: Option<TrainingSetup>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            link_code: String::new(),
            pending_generated_code: None,
            save_view: save_view::State::new(),
            save_action: SaveAction::None,
            last_error: None,
            save_body_enter: crate::anim::Enter::default(),
            save_form: crate::anim::Transition::swap(false),
            save_action_exit: SaveAction::None,
            training_setup: None,
        }
    }
}

/// Side-effects bubble-up. Mirrors the [`crate::tabs::replays::Effect`]
/// convention: pure UI-state mutations happen inside
/// [`State::update`]; anything that requires App-level
/// collaborators (session host, file system, clipboard) comes back
/// as an `Effect` for the caller to interpret.
#[derive(Debug)]
pub enum Effect {
    /// Kick off netplay. The `LinkIdent` variant tells the app
    /// handler whether to route via matchmaking signaling or direct
    /// TCP transport. `copy_code` is `Some` when Fight auto-generated
    /// the code for an empty input — the App puts it straight on the
    /// clipboard so the host can paste it to their opponent without
    /// hunting for the lobby's copy button.
    Connect {
        ident: crate::netplay::LinkIdent,
        copy_code: Option<String>,
    },
    /// Forward verbatim to the netplay subsystem.
    Netplay(crate::netplay::Message),
    /// Lobby frame-delay slider moved. App persists `config.frame_delay`; it's
    /// this side's local frame delay (snapshotted into the match at
    /// start, not negotiated with the peer), so there's nothing live to update.
    SetFrameDelay(u32),
    /// Lobby Ready — App reads the local save SRAM and
    /// dispatches `netplay::Message::Commit`.
    ReadyWithSave,
    /// `open::that(_)` on a file or folder.
    OpenPath(std::path::PathBuf),
    /// Copy plain text to the clipboard.
    CopyText(String),
    /// Copy a raster image to the clipboard.
    CopyImage(image::RgbaImage),
    /// User pressed Play → start a single-player session from the
    /// current selection.
    StartSinglePlayer,
    /// Training setup confirmed → boot a training session from the
    /// current selection plus these options.
    StartTraining(crate::session::training::TrainingOptions),
    /// Duplicate the currently-selected save file as `new_stem` (no
    /// extension; the handler keeps the source's).
    SaveDuplicate { new_stem: String },
    /// Rename the currently-selected save to `new_stem` (no
    /// extension; rename_save adds `.sav`).
    SaveRename { new_stem: String },
    /// Delete the currently-selected save file.
    SaveDelete,
    /// Create a fresh save in the saves dir from a bundled
    /// template.
    SaveNew {
        name: String,
        template: String,
        /// The concrete variant to create the save for (a family can
        /// have several owned-ROM variants). The handler looks up this
        /// game's template and adopts it as the loadout's game.
        game: rom::GameRef,
    },
    /// Task returned from save_view::State::apply. Generic pipe
    /// so save_view-internal side effects (e.g. the scroll-to-top
    /// snap on tab change) flow through without per-feature
    /// Effect variants.
    SaveViewTask(iced::Task<Message>),
    /// Save editor: stage one edit into the loaded save in memory
    /// (UI updates live; nothing hits disk yet).
    Edit(crate::save_edit::Edit),
    /// Global save editor: write every staged edit (folder + navicust +
    /// patch cards + auto battle data) to the .sav on disk in one shot.
    SaveEditCommit,
    /// Global save editor: discard all staged edits, reloading the on-disk
    /// original.
    SaveEditCancel,
}

impl State {
    /// Apply a tab message. See [`crate::tabs::replays::Effect`]
    /// for the side-effect surface convention.
    pub fn update(
        &mut self,
        msg: Message,
        scanners: &Scanners,
        config: &config::Config,
        loaded: Option<&selection::Loaded>,
        loadout: &Loadout,
    ) -> Option<Effect> {
        let action_before = self.save_action.clone();
        let effect = self.update_inner(msg, scanners, config, loaded, loadout);
        let now = iced::time::Instant::now();
        // Freeze the closing form (with its draft) for the
        // save-action swap's exit half to render.
        if self.save_action == SaveAction::None && action_before != SaveAction::None {
            self.save_action_exit = action_before;
        }
        self.save_form.set(self.save_action != SaveAction::None, now);
        effect
    }

    /// Move a Fight-generated link code into the input. The App calls this
    /// the moment the lobby band leaves (failure, cancel, or handoff into a
    /// match), so the returning strip shows what was hosted and a retry
    /// re-hosts the same code — without the code ever rendering in the
    /// input on the way *into* the lobby.
    pub fn restore_generated_link_code(&mut self) {
        if let Some(code) = self.pending_generated_code.take() {
            if self.link_code.trim().is_empty() {
                self.link_code = code;
            }
        }
    }

    /// Prefill the link-code input from an external source — the CLI
    /// `Join <code>` argument or a Discord join secret.
    pub fn adopt_link_code(&mut self, code: String) {
        self.link_code = code;
    }

    /// Surface an after-the-fact action failure (session launch / PvP
    /// build errored) as the tab's dismissable banner.
    pub fn set_error(&mut self, message: String) {
        self.last_error = Some(message);
    }

    /// Leave any in-progress save-edit session. The App calls this when
    /// the loaded save is rebuilt out from under the view, where staged
    /// edits (which lived in the previous in-memory save) are already
    /// gone — dropping the whole EditState clears every editor's
    /// scratch at once.
    pub fn reset_save_editing(&mut self) {
        self.save_view.clear_editing();
    }

    /// Play the family-switch entrance: a family change replaces the
    /// entire bottom of the tab, so the whole save-view pane under the
    /// selector strip glides in.
    pub fn animate_family_switch(&mut self, now: iced::time::Instant) {
        self.save_body_enter.start(now);
    }

    /// Play the save-switch entrance: a game/save change within the
    /// family only re-renders the save's content, so just the panes
    /// under the save view's sub-tab strip rise, leaving the strip
    /// planted.
    pub fn animate_save_switch(&mut self, now: iced::time::Instant) {
        self.save_view.enter_from = iced::Vector::new(0.0, 20.0);
        self.save_view.enter.start(now);
    }

    fn update_inner(
        &mut self,
        msg: Message,
        scanners: &Scanners,
        config: &config::Config,
        loaded: Option<&selection::Loaded>,
        loadout: &Loadout,
    ) -> Option<Effect> {
        match msg {
            // Routed to the shared Loadout at App level before this
            // dispatch is reached.
            Message::Loadout(_) => None,
            Message::LinkCodeChanged(s) => {
                // Direct-TCP commands (/host, /connect) need slashes,
                // spaces, dots, colons, brackets — pass them through.
                // Link codes are lowercased as typed: matchmaking is
                // case-sensitive, so this keeps a code read aloud or
                // retyped from a screenshot from missing its lobby.
                let filtered: String = if s.starts_with('/') {
                    s
                } else {
                    s.chars()
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                        .map(|c| c.to_ascii_lowercase())
                        .collect()
                };
                self.link_code = filtered.chars().take(100).collect();
                None
            }
            Message::CopyText(s) => {
                // The lobby's copy-link-code button is the only
                // sender — light its "Copied!" flash as the text
                // heads for the clipboard.
                crate::copy_feedback::flash(lobby::LINK_CODE_FLASH_KEY);
                Some(Effect::CopyText(s))
            }
            Message::FightPressed => {
                // An empty bar means "just get me a lobby": generate a
                // fresh random adjective-word-noun code and connect with
                // it directly. The code deliberately skips the input —
                // the outgoing strip still renders through the first
                // half of the band morph, so the lobby's connection
                // line (which masks in streamer mode) should be the
                // first place it shows. It rides the Connect effect
                // onto the clipboard, and `pending_generated_code`
                // refills the input once the lobby band leaves.
                let generated = self
                    .link_code
                    .trim()
                    .is_empty()
                    .then(|| crate::randomcode::generate(&config.language));
                // The Fight CTA is gated at the view layer to require
                // an empty or submittable link code, so reaching this
                // handler with a malformed one is a stale message +
                // safe to ignore.
                let ident = resolve_link_ident(generated.as_deref().unwrap_or(self.link_code.trim()))?;
                if let Some(code) = &generated {
                    self.pending_generated_code = Some(code.clone());
                    // Light the lobby copy button's "Copied!" flash as
                    // the band rises — the cue that the code is already
                    // in hand.
                    crate::copy_feedback::flash(lobby::LINK_CODE_FLASH_KEY);
                }
                // Clear any leftover after-the-fact error from a prior
                // attempt — the new attempt's outcome will replace it.
                self.last_error = None;
                Some(Effect::Connect {
                    ident,
                    copy_code: generated,
                })
            }
            Message::Noop => None,
            Message::Disconnect => Some(Effect::Netplay(crate::netplay::Message::Disconnect)),
            Message::SetMatchType(mt) => Some(Effect::Netplay(crate::netplay::Message::SetMatchType(mt))),
            Message::SetFrameDelay(d) => Some(Effect::SetFrameDelay(d)),
            Message::SetBlindSetup(v) => Some(Effect::Netplay(crate::netplay::Message::SetBlindSetup(v))),
            Message::Ready => Some(Effect::ReadyWithSave),
            Message::Unready => Some(Effect::Netplay(crate::netplay::Message::Uncommit)),
            Message::SaveViewAction(action) => {
                // `apply` folds the action into save-view state and hands
                // back what's left for the App: staged edits, clipboard
                // copies, launches. Everything else flows through as a
                // generic save-view-internal task.
                let (sv_task, outcome) = self.save_view.apply(&action, &config.language, loaded);
                match outcome {
                    Some(save_view::Outcome::Edit(edit)) => Some(Effect::Edit(edit)),
                    Some(save_view::Outcome::CopyText(s)) => Some(Effect::CopyText(s)),
                    Some(save_view::Outcome::CopyImage(img)) => Some(Effect::CopyImage(img)),
                    Some(save_view::Outcome::Play) => {
                        // Clear stale error from a prior attempt; the
                        // new launch's outcome takes its place.
                        self.last_error = None;
                        Some(Effect::StartSinglePlayer)
                    }
                    Some(save_view::Outcome::Train) => {
                        self.last_error = None;
                        self.training_setup = loaded.map(TrainingSetup::for_selection);
                        None
                    }
                    Some(save_view::Outcome::Commit) => Some(Effect::SaveEditCommit),
                    Some(save_view::Outcome::Cancel) => Some(Effect::SaveEditCancel),
                    None => Some(Effect::SaveViewTask(sv_task.map(Message::SaveViewAction))),
                }
            }
            Message::DismissError => {
                self.last_error = None;
                None
            }
            Message::TrainingSetup(m) => self.update_training_setup(m),
            m @ (Message::SaveOpenFolder
            | Message::OpenSavesFolder(_)
            | Message::SaveDuplicateStart
            | Message::SaveDuplicateDraftChanged(_)
            | Message::SaveDuplicateConfirm
            | Message::SaveRenameStart
            | Message::SaveRenameDraftChanged(_)
            | Message::SaveRenameConfirm
            | Message::SaveDeleteStart
            | Message::SaveDeleteConfirm
            | Message::SaveActionCancel
            | Message::SaveNewStart
            | Message::SaveNewDraftChanged(_)
            | Message::SaveNewTemplateSelected(..)
            | Message::SaveNewConfirm) => self.update_save_manage(m, scanners, config, loadout),
        }
    }
}

/// The netplay-side inputs the Play tab needs to render its bottom
/// band, bundled so [`State::view`] doesn't take the App's netplay
/// internals as loose positional arguments.
pub struct LobbyBandCtx<'a> {
    pub phase: &'a crate::netplay::Phase,
    pub lobby: &'a crate::netplay::LobbyState,
    /// True between "both sides exchanged StartMatch" and the PvP
    /// session taking over: the selector strip goes inert and the
    /// lobby shows its "Starting match…" chrome.
    pub handoff_pending: bool,
    /// Two-phase swap between the bottom bands (link-code strip ↔
    /// lobby), driven by the App (which sees the netplay phase
    /// flip): first half sinks + dissolves the outgoing band,
    /// second half rises the incoming one out of the page surface.
    pub swap: &'a crate::anim::Transition,
    /// The lobby's last live state, frozen by the App on the
    /// frame the band left — the exiting band renders from
    /// this so the verdict (e.g. the failure banner) doesn't
    /// flash to the idle handshake line mid-dissolve.
    pub exit_snapshot: Option<&'a (crate::netplay::Phase, crate::netplay::LobbyState)>,
}

impl State {
    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loadout: &'a Loadout,
        loaded: Option<&'a selection::Loaded>,
        streamer_mode: bool,
        config: &'a config::Config,
        band: LobbyBandCtx<'a>,
    ) -> Element<'a, Message> {
        let now = iced::time::Instant::now();
        let mut save_body = self.body(lang, scanners, loadout, loaded, streamer_mode, config, band.phase);
        // A family switch replaces the entire bottom of the tab, so
        // the whole pane glides in (the App starts `save_body_enter`
        // when `loadout.family` flips); save switches within the
        // family animate inside the save view instead.
        if let Some(p) = self.save_body_enter.progress(now) {
            save_body = crate::anim::slide_in(save_body, p, iced::Vector::new(0.0, 20.0));
        }

        // Selector strip + save-view body live inside a single
        // PANE_GAP-padded column so every pane in that area shares
        // the same inset from the window edges and gap from one
        // another. The hud_scanline_bottom + bottom band sit OUTSIDE
        // that padding so they remain edge-to-edge bottom bars.
        // The strip goes inert during the handoff window: the PvP
        // session is being built from the committed state and
        // selection changes would only confuse.
        let inner = column![
            self.selector_strip(lang, scanners, loadout, config, band.handoff_pending),
            save_body,
        ]
        .spacing(style::PANE_GAP)
        .padding(style::PANE_GAP)
        .height(Fill);

        let mut col = column![].width(Fill).height(Fill);
        if let Some(err) = &self.last_error {
            col = col.push(widgets::error_banner(lang, err, Message::DismissError));
        }
        col = col.push(inner);
        // While a netplay attempt is in flight (Connecting /
        // Negotiating / Lobby, sticky Failed, handoff) the lobby IS
        // the bottom band — it carries the versus cards, match
        // settings, and the verdict/cancel/ready chrome, while the
        // save view above stays visible. Otherwise the normal bottom
        // strip handles the link code + Fight CTA.
        //
        // The swap between them is a two-phase morph on
        // `bottom_swap`'s unified timeline: the outgoing band sinks
        // while dissolving into the page surface, then the incoming
        // one rises out of it — so the code strip reads as turning
        // into the lobby and back, rather than one vanishing and the
        // other arriving. The bottom HUD scanline is grouped into the
        // moving band so it rides the motion instead of staying
        // pinned above it.
        let (render_lobby, swap) = crate::anim::swap_phase(band.swap, now);
        let bottom: Element<'a, Message> = if render_lobby {
            // While the band is on its way OUT, the live phase has
            // already gone Idle (and the lobby may be wiped) — use
            // the snapshot the App froze on the band's last live
            // frame so the verdict doesn't flash mid-dissolve.
            let (band_phase, band_lobby) = if !band.swap.shown() {
                band.exit_snapshot
                    .map(|(p, l)| (p, l))
                    .unwrap_or((band.phase, band.lobby))
            } else {
                (band.phase, band.lobby)
            };
            // Synthesize the local side's Settings from the current
            // loadout so the "You" slot fills in immediately —
            // pre-Lobby phases haven't populated `lobby.local` yet,
            // but everything it needs is already on hand locally.
            // Same builder the netplay loop uses to ship settings on
            // the wire, so the visible info during the handshake
            // exactly matches what gets sent.
            let local_fallback = loadout.make_local_settings(config, band.lobby);
            lobby::Lobby {
                lang,
                state: band_lobby,
                phase: band_phase,
                local_game: loadout.game,
                scanners,
                has_save: loaded.is_some(),
                local_fallback,
                streamer_mode,
                handoff_pending: band.handoff_pending,
                frame_delay: config.frame_delay,
            }
            .view()
        } else {
            self.bottom_strip(lang, streamer_mode)
        };
        let mut group: Element<'a, Message> = column![widgets::hud_scanline_bottom(), bottom].width(Fill).into();
        if let Some(phase) = swap {
            let dist = if render_lobby { 48.0 } else { 24.0 };
            group = crate::anim::swap_transform(group, phase, iced::Vector::new(0.0, dist), |theme: &iced::Theme| {
                theme.palette().background
            });
        }
        col = col.push(group);
        // Training setup rides above the whole tab as a modal while a
        // draft is open (its Start/Cancel/backdrop all close it).
        if let (Some(setup), Some(loaded)) = (self.training_setup.as_ref(), loaded) {
            return iced::widget::stack![
                Element::from(col),
                self.training_setup_modal(lang, scanners, loaded, setup)
            ]
            .into();
        }
        col.into()
    }

    /// Picks between the save view, an empty-state hint, or a "pick a
    /// save" hint based on what the user has installed and selected.
    fn body<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loadout: &'a Loadout,
        loaded: Option<&'a selection::Loaded>,
        streamer_mode: bool,
        config: &'a config::Config,
        netplay_phase: &'a crate::netplay::Phase,
    ) -> Element<'a, Message> {
        // No ROMs at all: explain where to put them.
        if scanners.roms.read().is_empty() {
            let roms_path = config.roms_path();
            return empty_state_card(
                t!(lang, "empty-no-roms-title"),
                vec![t!(lang, "empty-no-roms-body"), roms_path.display().to_string()],
                Some((t!(lang, "save-open-folder"), roms_path)),
            );
        }
        // Family selected but no save files anywhere in it.
        if let Some(family) = loadout.family {
            let saves = scanners.saves.read();
            let has_saves =
                game::games_in_family(family).any(|g| saves.get(&g).map(|v| !v.is_empty()).unwrap_or(false));
            if !has_saves && loadout.save.is_none() {
                let saves_path = config.saves_path();
                return empty_state_card(
                    t!(lang, "empty-no-saves-title"),
                    vec![t!(lang, "empty-no-saves-body"), saves_path.display().to_string()],
                    Some((t!(lang, "save-open-folder"), saves_path)),
                );
            }
        }
        self.save_view(lang, loaded, streamer_mode, netplay_phase)
    }

    fn selector_strip<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loadout: &'a Loadout,
        config: &'a config::Config,
        inert: bool,
    ) -> Element<'a, Message> {
        // Inert during the PvP handoff window — the loadout pickers
        // reroute to Noop so a mid-spawn selection change can't
        // contradict the committed state, without the strip changing
        // shape.
        let gate = move |m: loadout::Message| if inert { Message::Noop } else { Message::Loadout(m) };
        let game_row: Element<'a, Message> = loadout::game_row(loadout, lang, scanners, config).map(gate);
        let save_picker: Element<'a, Message> =
            Element::from(loadout::save_picker(loadout, lang, scanners, config).width(Length::Fill)).map(gate);
        let save_row = self.save_action_row(lang, scanners, loadout, save_picker);

        container(column![game_row, save_row].spacing(6))
            .padding(style::PANE_PADDING)
            .width(Fill)
            .style(widgets::pane)
            .into()
    }

    fn save_view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        loaded: Option<&'a selection::Loaded>,
        streamer_mode: bool,
        netplay_phase: &'a crate::netplay::Phase,
    ) -> Element<'a, Message> {
        let Some(loaded) = loaded else {
            return container(text(t!(lang, "play-no-selection")).size(TEXT_BODY))
                .center(Fill)
                .into();
        };
        // Play button is the singleplayer entry point — disabled
        // whenever a netplay attempt is anywhere in flight so it
        // can't fight with the lobby for the same save/emulator slot.
        let play_button = Some(matches!(netplay_phase, crate::netplay::Phase::Idle));
        save_view::view(
            lang,
            loaded,
            &self.save_view,
            streamer_mode,
            play_button,
            true,
            // The save editor is always available (no longer experimental).
            true,
        )
        .map(Message::SaveViewAction)
    }

    /// The idle bottom band: link-code input + Fight CTA. The lobby
    /// band replaces this strip for every in-flight netplay phase, so
    /// this strip is pure "enter a link code and fight" —
    /// singleplayer lives in the save view's Play button. An empty
    /// input is fair game: Fight generates a random code on the spot
    /// (see the FightPressed handler) which never renders here on the
    /// way in — it debuts in the lobby's connection line and only
    /// lands in this input when the lobby band leaves. In streamer
    /// mode the input renders secure (masked) — typed, pasted, and
    /// restored-generated codes are all scrapeable off a stream
    /// otherwise.
    fn bottom_strip<'a>(&'a self, lang: &'a LanguageIdentifier, streamer_mode: bool) -> Element<'a, Message> {
        const BOTTOM_SIZE: f32 = 15.0;
        const BOTTOM_PAD: [f32; 2] = [10.0, 16.0];
        const BOTTOM_CTA_PAD: [f32; 2] = [10.0, 22.0];
        let trimmed = self.link_code.trim();
        let can_submit = trimmed.is_empty() || resolve_link_ident(trimmed).is_some();
        let fight_button: Element<'a, Message> = {
            // Same chrome as the lobby's Ready button — both are
            // "commit to a match" CTAs. ready_button_style for
            // ReadyPalette::Idle falls back to neutral when the
            // button is disabled, so the empty-link-code case
            // renders as a plain greyed-out pill without a
            // separate branch here.
            let label = row![
                Icon::Swords.widget().size(BOTTOM_SIZE),
                text(t!(lang, "play-fight")).size(BOTTOM_SIZE),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            let mut btn = button(label)
                .padding(BOTTOM_CTA_PAD)
                .height(Length::Fixed(crate::style::BAR_CONTROL_HEIGHT))
                .style(|theme: &iced::Theme, status| ready_button_style(theme, status, ReadyPalette::Idle));
            if can_submit {
                btn = btn.on_press(Message::FightPressed);
            }
            btn.into()
        };
        // Link-code input fills all the slack to the left of the
        // Fight CTA. text_input doesn't expose a `.height()` method,
        // so we wrap it in a fixed-height container to match the
        // surrounding controls.
        let link_input: Element<'a, Message> = container(
            text_input(&t!(lang, "play-link-code"), &self.link_code)
                .secure(streamer_mode)
                .on_input(Message::LinkCodeChanged)
                .on_submit(Message::FightPressed)
                .size(BOTTOM_SIZE)
                .padding(BOTTOM_PAD)
                .width(Length::Fill)
                .style(widgets::chunky_text_input),
        )
        .height(Length::Fixed(crate::style::BAR_CONTROL_HEIGHT))
        .width(Length::Fill)
        .into();

        container(
            row![link_input, fight_button]
                .spacing(10)
                .align_y(Alignment::Center)
                .padding([10, 8]),
        )
        .width(Fill)
        .style(widgets::hud_bar)
        .into()
    }
}

// ---------- Save-action form helpers ----------

/// Centered card used for the no-roms / no-saves hints. Title is
/// rendered larger, body lines stack underneath in muted text.
/// When `folder` is provided, appends an "Open Folder" button —
/// usually the same path as the body's last line, so the user
/// can click straight through instead of copy-pasting it into
/// their file manager.
fn empty_state_card(
    title: String,
    body_lines: Vec<String>,
    open_folder: Option<(String, std::path::PathBuf)>,
) -> Element<'static, Message> {
    let mut col = column![
        // Lucide "info" glyph sized up so the card has a clear
        // visual anchor — without it the empty state was just a
        // floating title + paragraph, which read as a flash of
        // text rather than a deliberate placeholder.
        Icon::Info.widget().size(28.0),
        text(title).size(TEXT_TITLE),
    ]
    .spacing(10)
    .align_x(Alignment::Center);
    for line in body_lines {
        col = col.push(text(line).size(TEXT_CAPTION).style(widgets::muted_text_style));
    }
    if let Some((label, path)) = open_folder {
        col = col.push(Space::new().height(4)).push(widgets::labeled_icon_button(
            Icon::Folder,
            label,
            Message::OpenSavesFolder(path),
            STANDARD_PADDING,
            widgets::neutral,
        ));
    }
    container(container(col.padding(28).max_width(520)).style(widgets::panel))
        .padding(24)
        .center(Fill)
        .into()
}

// ---------- File-level save helpers ----------

/// Which ready-button state we're painting. Drives
/// [`ready_button_style`]'s color choice.
#[derive(Clone, Copy)]
enum ReadyPalette {
    /// Pre-commit; the action is "ready up". Accent (primary) so
    /// it reads as the call-to-action in the strip.
    Idle,
    /// Locally committed; the action is "unready". Neutral / gray —
    /// the commitment isn't a celebration to surface in green;
    /// what matters is the user can un-commit.
    Committed,
    /// Both committed; match is spinning up. Rendered as a passive
    /// indicator: muted background, no click target, no border.
    /// Caller sets `on_press = None` to match the disabled look.
    Starting,
}

/// Custom style for the lobby's Ready toggle. Three discrete
/// moods — each one its own visual register so a glance at the
/// button tells the whole story of "where are we in the
/// handshake".
///
/// * Idle — primary_button on steroids: brighter gradient, huge
///   primary glow, chunky 2 px border. This is the moment the user is
///   supposed to slam the button, so it has to feel hot.
/// * Committed — neutral beveled plate. We've ack'd locally and are
///   waiting on the peer; the only useful action is to take it back,
///   which is not a celebration.
/// * Starting — flat muted badge. Both sides committed; the button is
///   now purely a status indicator with no click target.
fn ready_button_style(theme: &iced::Theme, status: button::Status, palette: ReadyPalette) -> button::Style {
    let p = theme.extended_palette();
    let primary = theme.palette().primary;
    match palette {
        ReadyPalette::Starting => button::Style {
            background: Some(iced::Background::Color(p.background.weak.color)),
            text_color: widgets::muted_color(theme),
            border: iced::Border {
                radius: widgets::tech_radius(10.0),
                width: 1.0,
                color: p.background.strong.color,
            },
            ..Default::default()
        },
        ReadyPalette::Committed => {
            // Defer to the shared beveled neutral so the
            // un-ready toggle looks like a sibling of the other
            // chunky neutral buttons in the lobby strip.
            crate::widgets::neutral(theme, status)
        }
        ReadyPalette::Idle => {
            // Disabled state defers to the standard neutral
            // button so it reads as a plainly-greyed-out button
            // — the dim-primary-fill version this used to
            // render looked like a corrupted variant of the
            // lit-up state rather than a disabled affordance.
            if matches!(status, button::Status::Disabled) {
                return crate::widgets::neutral(theme, status);
            }
            // Inline expansion of the battle-button kernel with
            // every dial cranked: bigger glow, brighter top stop,
            // 2 px border so the button reads as a console
            // affordance rather than a CSS rectangle.
            let lighter = widgets::mix(primary, iced::Color::WHITE, 0.30);
            let darker = widgets::mix(primary, iced::Color::BLACK, 0.25);
            let (top, bottom, glow_alpha, offset_y, blur) = match status {
                button::Status::Hovered => (
                    widgets::mix(lighter, iced::Color::WHITE, 0.18),
                    widgets::mix(primary, iced::Color::WHITE, 0.05),
                    0.95,
                    8.0,
                    28.0,
                ),
                button::Status::Pressed => (darker, widgets::mix(darker, iced::Color::BLACK, 0.12), 0.35, 2.0, 14.0),
                button::Status::Disabled => unreachable!("handled above"),
                button::Status::Active => (lighter, darker, 0.75, 6.0, 22.0),
            };
            button::Style {
                background: Some(iced::Background::Gradient(iced::Gradient::Linear(
                    iced::gradient::Linear::new(0.0)
                        .add_stop(0.0, top)
                        .add_stop(1.0, bottom),
                ))),
                text_color: widgets::on_accent(primary),
                border: iced::Border {
                    radius: widgets::tech_radius(10.0),
                    width: 2.0,
                    color: widgets::mix(primary, iced::Color::WHITE, 0.45),
                },
                shadow: iced::Shadow {
                    color: iced::Color {
                        a: glow_alpha,
                        ..primary
                    },
                    offset: iced::Vector::new(0.0, offset_y),
                    blur_radius: blur,
                },
                snap: false,
            }
        }
    }
}

// ---------- Link-code parsing ----------

/// Resolve a trimmed link-code input into a submittable
/// [`crate::netplay::LinkIdent`], or `None` if the input isn't
/// submittable (empty, or a malformed `/`-prefixed direct command).
fn resolve_link_ident(input: &str) -> Option<crate::netplay::LinkIdent> {
    if input.is_empty() {
        return None;
    }
    if input.starts_with('/') {
        parse_direct_command(input).map(crate::netplay::LinkIdent::Direct)
    } else {
        Some(crate::netplay::LinkIdent::Matchmaking(input.to_string()))
    }
}

/// Recognise the direct-TCP link-code commands the user can type
/// in place of a matchmaking code:
///
/// - `/host` — listen on [`crate::net::DEFAULT_LOCAL_PORT`]
/// - `/host <port>` — listen on the given port
/// - `/connect <addr>` — dial `<addr>`, appending the default port if
///   the user didn't specify one
fn parse_direct_command(input: &str) -> Option<crate::netplay::DirectRole> {
    // The leading slash is the disambiguator — without it, any
    // input is a matchmaking link code (which can legitimately
    // contain letters, digits, and the random-code separators).
    if !input.starts_with('/') {
        return None;
    }
    let mut parts = input.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().map(str::trim).unwrap_or("");
    match cmd {
        "/host" => {
            let port = if arg.is_empty() {
                crate::net::DEFAULT_LOCAL_PORT
            } else {
                arg.parse::<u16>().ok()?
            };
            Some(crate::netplay::DirectRole::Host { port })
        }
        "/connect" => {
            if arg.is_empty() {
                return None;
            }
            // Heuristic: if the user gave no colon (bare IP) or
            // their input ends with the IPv6 closing bracket
            // without a trailing colon, append the default port.
            // We deliberately don't try to validate the address
            // itself — TcpStream::connect's error surfaces well.
            let addr = if arg.contains(':') && !arg.ends_with(']') {
                arg.to_string()
            } else {
                format!("{arg}:{}", crate::net::DEFAULT_LOCAL_PORT)
            };
            Some(crate::netplay::DirectRole::Connect { addr })
        }
        _ => None,
    }
}
