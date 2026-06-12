//! The Play tab: everything in one place. The full loadout selector
//! (family / save / patch pickers + save management) and the save
//! viewer/editor fill the body, and a netplay band rides the bottom —
//! the link-code strip + Fight CTA when idle, the full lobby
//! ([`lobby`]) once a connection attempt is in flight. The save view
//! stays on screen through it all, so what you're bringing to the
//! match is always visible (and switchable) even mid-lobby. The
//! selection state itself is App-level ([`crate::loadout::Loadout`])
//! so the lobby settings-resend sees every change made here live.

mod lobby;

use crate::app::Scanners;
use crate::i18n::t;
use crate::loadout::{self, Loadout};
use crate::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_TITLE};
use crate::widgets;
use crate::{config, game, rom, save_view, selection};
use iced::widget::{button, container, text, Space};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, pick_list, row, text_input};
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
    /// Lobby UI: user toggled the reveal-setup checkbox.
    SetRevealSetup(bool),
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
}

// ---------- Play tab state ----------

pub struct State {
    pub link_code: String,
    /// Persistent state for the embedded save view (active tab,
    /// folder grouping). Apply incoming `SaveViewAction`s via
    /// [`save_view::State::apply`].
    pub save_view: save_view::State,
    /// Inline state for the save-management actions (rename / delete).
    pub save_action: SaveAction,
    /// Last after-the-fact action failure (singleplayer launch
    /// errored, PvP session build failed, …) — rendered as a
    /// dismissable banner at the top of the tab. Pre-condition errors
    /// ("you need a save first") are NOT funneled here; they're
    /// handled by view-time button gating + inline hints, because
    /// graying out the action surface explains itself.
    pub last_error: Option<String>,
    /// Entrance glide for the whole save-view pane under the selector
    /// strip, played by the App when the *family* changes — a family
    /// switch replaces the entire bottom of the tab. A save switch
    /// within the family plays the smaller [`save_view::State`]
    /// entrance instead, which only moves the panes under the save
    /// view's sub-tab strip.
    pub save_body_enter: crate::anim::Enter,
    /// Fade-through swap for the save-action row: the picker row
    /// morphs into whichever rename / delete / create form opens
    /// ([`State::save_action`]) and back.
    pub save_form: crate::anim::Transition,
    /// The form that was open before `save_action` reset to
    /// `None`, frozen so the swap's exit half has something to
    /// render (the live form — including the rename draft — is
    /// already gone).
    pub save_action_exit: SaveAction,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub enum SaveAction {
    #[default]
    None,
    Renaming {
        draft: String,
    },
    /// Duplicating the selected save. `draft` is the new file's name,
    /// prefilled with the next free "<stem> (copy)" suggestion so a
    /// plain Enter behaves like the old one-click duplicate.
    Duplicating {
        draft: String,
    },
    ConfirmDelete,
    /// Creating a new save. `template` is the template name (empty
    /// string is the default unnamed template); `draft` is the user's
    /// chosen filename. `game` is the concrete variant the save is
    /// created for — chosen together with the template, since within a
    /// family the same template name exists per color (White/Blue), and
    /// the new file must carry the right variant signature.
    /// `template`/`game` stay `None` until the user picks (auto-selected
    /// when only one option exists). The Confirm button is disabled in
    /// that state — there's no "default" template to fall back on.
    NewSave {
        draft: String,
        game: Option<rom::GameRef>,
        template: Option<String>,
        /// The auto-generated default we last wrote into `draft`. While
        /// the user hasn't typed over it, switching templates regenerates
        /// the suggestion; once they edit it, this is `None` and we leave
        /// their value alone.
        auto_default: Option<String>,
    },
}

impl Default for State {
    fn default() -> Self {
        Self {
            link_code: String::new(),
            save_view: save_view::State::new(),
            save_action: SaveAction::None,
            last_error: None,
            save_body_enter: crate::anim::Enter::default(),
            save_form: crate::anim::Transition::swap(false),
            save_action_exit: SaveAction::None,
        }
    }
}

/// A single folder edit staged by the folder editor. Applied to the
/// loaded save in memory by [`Effect::EditChips`]; not persisted to
/// disk until the user hits Save ([`Effect::SaveEditCommit`]).
#[derive(Debug, Clone)]
pub enum ChipEdit {
    /// Add chip `chip_id` with `code` to the first empty folder slot.
    AddChip {
        chip_id: usize,
        code: tango_dataview::save::ChipCode,
    },
    /// Empty `slot`.
    RemoveChip { slot: usize },
    /// Reorder: move the chip at `from` to `to` (an ordered move that shifts
    /// the chips in between). Both slots must be filled — the editor never
    /// drags an empty slot or drops into a gap. REG/TAG slot pointers follow
    /// the moved chips.
    MoveChip { from: usize, to: usize },
    /// Empty every folder slot (and clear REG/TAG).
    ClearFolder,
    /// Toggle `slot` as the folder's Regular chip (clear if already set).
    ToggleRegular { slot: usize },
    /// Set (or clear, with `None`) the folder's Tag chip pair.
    SetTags(Option<[usize; 2]>),
}

/// A single navicust edit staged by the navicust editor. Applied to the
/// loaded save in memory by [`Effect::EditNavicust`]; not persisted to
/// disk until the user hits Save ([`Effect::SaveEditCommit`]).
#[derive(Debug, Clone)]
pub enum NavicustEdit {
    /// Place a part into the first empty navicust slot.
    AddPart(tango_dataview::save::NavicustPart),
    /// Empty navicust slot `slot`.
    RemovePart { slot: usize },
    /// Remove every installed part.
    ClearAll,
}

/// A single BN5/BN6 patch-card edit staged by the editor. Applied to the
/// loaded save in memory by [`Effect::EditPatchCard56s`]; not persisted to
/// disk until the user hits Save ([`Effect::SaveEditCommit`]).
#[derive(Debug, Clone)]
pub enum PatchCard56Edit {
    /// Register patch card `id` (append to the list, enabled).
    AddCard { id: usize },
    /// Unregister the patch card in `slot` (shift the rest up).
    RemoveCard { slot: usize },
    /// Reorder: move the card at `from` to `to` (an ordered move that shifts
    /// the cards in between). The registered list is dense, so both ends are
    /// always valid.
    MoveCard { from: usize, to: usize },
    /// Toggle the patch card in `slot` between enabled and disabled.
    ToggleCard { slot: usize },
    /// Unregister every patch card.
    ClearAll,
}

/// A single BN4 patch-card edit staged by the editor. Applied to the loaded
/// save in memory by [`Effect::EditPatchCard4s`]; not persisted to disk
/// until the user hits Save ([`Effect::SaveEditCommit`]). BN4 is slot-based:
/// every card belongs to one fixed catalog slot (0A–0F), so adding a card
/// installs it into its own slot (replacing whatever was there).
#[derive(Debug, Clone)]
pub enum PatchCard4Edit {
    /// Install patch card `id` into its own catalog slot, enabled.
    AddCard { id: usize },
    /// Clear catalog slot `slot`.
    RemoveCard { slot: usize },
    /// Toggle slot `slot`'s card between enabled and disabled.
    ToggleCard { slot: usize },
    /// Clear every slot.
    ClearAll,
}

/// A single auto-battle-data edit staged by the editor. Applied to the
/// loaded save in memory by [`Effect::EditAutoBattleData`]; not persisted
/// to disk until the user hits Save ([`Effect::SaveEditCommit`]). The deck
/// is derived from per-chip use counts, so these set those counts; the
/// applier rebuilds the materialized deck after each so the preview shows
/// the change live.
#[derive(Debug, Clone)]
pub enum AutoBattleDataEdit {
    /// Set chip `id`'s primary use count.
    SetUseCount { id: usize, count: usize },
    /// Set chip `id`'s secondary use count (Standard chips only).
    SetSecondaryUseCount { id: usize, count: usize },
    /// Zero every chip's use counts, emptying the deck.
    ClearAll,
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
    /// TCP transport.
    Connect(crate::netplay::LinkIdent),
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
    /// Folder editor: stage one edit into the loaded save in memory
    /// (UI updates live; nothing hits disk yet).
    EditChips(ChipEdit),
    /// Navicust editor: stage one edit into the loaded save in memory
    /// (UI updates live; nothing hits disk yet).
    EditNavicust(NavicustEdit),
    /// BN5/BN6 patch-card editor: stage one edit into the loaded save in
    /// memory (UI updates live; nothing hits disk yet).
    EditPatchCard56s(PatchCard56Edit),
    /// BN4 patch-card editor: stage one edit into the loaded save in memory
    /// (UI updates live; nothing hits disk yet).
    EditPatchCard4s(PatchCard4Edit),
    /// Auto-battle-data editor: stage one edit into the loaded save in
    /// memory (UI updates live; nothing hits disk yet).
    EditAutoBattleData(AutoBattleDataEdit),
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
                // fresh random adjective-word-noun code, drop it into
                // the input so the user can see what they're hosting
                // on, and connect with it — the lobby subtitle's copy
                // button hands it to the opponent from there.
                if self.link_code.trim().is_empty() {
                    self.link_code = crate::randomcode::generate(&config.language);
                }
                // The Fight CTA is gated at the view layer to require
                // an empty or submittable link code, so reaching this
                // handler with a malformed one is a stale message +
                // safe to ignore.
                let ident = resolve_link_ident(self.link_code.trim())?;
                // Clear any leftover after-the-fact error from a prior
                // attempt — the new attempt's outcome will replace it.
                self.last_error = None;
                Some(Effect::Connect(ident))
            }
            Message::Noop => None,
            Message::Disconnect => Some(Effect::Netplay(crate::netplay::Message::Disconnect)),
            Message::SetMatchType(mt) => Some(Effect::Netplay(crate::netplay::Message::SetMatchType(mt))),
            Message::SetFrameDelay(d) => Some(Effect::SetFrameDelay(d)),
            Message::SetRevealSetup(v) => Some(Effect::Netplay(crate::netplay::Message::SetRevealSetup(v))),
            Message::Ready => Some(Effect::ReadyWithSave),
            Message::Unready => Some(Effect::Netplay(crate::netplay::Message::Uncommit)),
            Message::SaveViewAction(action) => {
                use save_view::Action as A;
                let sv_task = self.save_view.apply(&action);
                match action {
                    A::CopyTab(tab) => {
                        let opts = save_view::RenderOpts {
                            folder_grouped: self.save_view.folder_grouped,
                        };
                        let effect = loaded
                            .and_then(|l| save_view::tab_as_text(&config.language, tab, l, opts))
                            .map(Effect::CopyText);
                        // Only a copy that actually produced text
                        // earns the "Copied!" flash.
                        if effect.is_some() {
                            crate::copy_feedback::flash(&save_view::copy_flash_key(tab, false));
                        }
                        effect
                    }
                    A::CopyTabImage(tab) => {
                        let effect = loaded
                            .and_then(|l| save_view::tab_as_image(tab, l))
                            .map(Effect::CopyImage);
                        if effect.is_some() {
                            crate::copy_feedback::flash(&save_view::copy_flash_key(tab, true));
                        }
                        effect
                    }
                    A::PlayClicked => {
                        // Clear stale error from a prior attempt; the
                        // new launch's outcome takes its place.
                        self.last_error = None;
                        Some(Effect::StartSinglePlayer)
                    }
                    // ----- Folder editor -----
                    // EnterEdit needs the read view to seed tag state +
                    // build the per-slot chip pickers, so it touches
                    // save_view state directly rather than emitting an
                    // Effect.
                    A::EnterEdit => {
                        if let Some(l) = loaded {
                            self.save_view.enter_edit(l);
                        }
                        None
                    }
                    // One global Save / Cancel for the whole save.
                    A::SaveEdit => Some(Effect::SaveEditCommit),
                    A::CancelEdit => Some(Effect::SaveEditCancel),
                    A::AddChip { chip_id, code } => {
                        // New chips are inserted at the top, sliding the
                        // existing run down into the first empty slot — so
                        // shift the staged TAG selection to match.
                        if let Some(gap) = loaded.and_then(|l| l.save.view_chips()).and_then(|v| {
                            let fi = v.equipped_folder_index();
                            (0..save_view::MAX_FOLDER_CHIPS).find(|&i| v.chip(fi, i).is_none())
                        }) {
                            self.save_view.shift_tags_for_top_insert(gap);
                        }
                        Some(Effect::EditChips(ChipEdit::AddChip { chip_id, code }))
                    }
                    A::RemoveChip { slot } => {
                        // Mirror the save-side compaction in the in-progress
                        // tag selection (drop + shift), so the TAG toggles
                        // stay aligned with the shifted chips.
                        self.save_view.compact_tags(slot);
                        Some(Effect::EditChips(ChipEdit::RemoveChip { slot }))
                    }
                    A::ReorderChips(ev) => {
                        // Only a completed drop reorders; pick-up / cancel are
                        // visual-only.
                        use sweeten::widget::drag::DragEvent;
                        let DragEvent::Dropped { index, target_index } = ev else {
                            return None;
                        };
                        let from = index;
                        // Live folder occupancy, to validate + resolve the drop.
                        let Some(filled) = loaded.and_then(|l| l.save.view_chips()).map(|v| {
                            let fi = v.equipped_folder_index();
                            (0..save_view::MAX_FOLDER_CHIPS)
                                .map(|i| v.chip(fi, i).is_some())
                                .collect::<Vec<bool>>()
                        }) else {
                            return None;
                        };
                        // Can't drag an empty slot.
                        if !filled.get(from).copied().unwrap_or(false) {
                            return None;
                        }
                        // Dropping onto an empty slot drops the chip in at the
                        // end of the packed list (the first empty slot above the
                        // target = right after the last chip), never leaving a gap.
                        let to = if filled.get(target_index).copied().unwrap_or(false) {
                            target_index
                        } else {
                            match filled.iter().rposition(|&f| f) {
                                Some(last) => last,
                                None => return None,
                            }
                        };
                        if from == to {
                            return None;
                        }
                        // Keep the staged TAG selection aligned with the move.
                        self.save_view.move_tags(from, to);
                        Some(Effect::EditChips(ChipEdit::MoveChip { from, to }))
                    }
                    A::ClearFolder => {
                        if let Some(e) = self.save_view.editing.as_mut() {
                            e.tags.clear();
                        }
                        Some(Effect::EditChips(ChipEdit::ClearFolder))
                    }
                    A::ToggleRegular { slot } => Some(Effect::EditChips(ChipEdit::ToggleRegular { slot })),
                    A::ToggleTag { slot } => {
                        // `toggle_tag` updates the in-progress UI
                        // selection and hands back the pair to commit
                        // (Some([a,b]) at two, else None to clear).
                        let pair = self.save_view.toggle_tag(slot);
                        Some(Effect::EditChips(ChipEdit::SetTags(pair)))
                    }
                    // ----- Navicust editor -----
                    A::PlaceHeld { col, row } => {
                        // Build the part from the held state (already
                        // folded by `apply`), then drop it so the cursor
                        // is free again.
                        let edit = self.save_view.editing.as_mut();
                        let part = edit.and_then(|e| {
                            let p = e.held_part.map(|h| tango_dataview::save::NavicustPart {
                                id: h.id,
                                col,
                                row,
                                rot: h.rot,
                                compressed: h.compressed,
                            });
                            e.held_part = None;
                            p
                        });
                        part.map(|p| Effect::EditNavicust(NavicustEdit::AddPart(p)))
                    }
                    A::PickUpInstalledPart { slot, col, row } => {
                        // Read the part being removed so it becomes the
                        // held part — the user can re-place / rotate it.
                        let held = loaded.and_then(|l| {
                            if let Some(tango_dataview::save::NaviView::Navicust(v)) = l.save.view_navi() {
                                v.navicust_part(slot)
                            } else {
                                None
                            }
                        });
                        if let (Some(p), Some(e)) = (held, self.save_view.editing.as_mut()) {
                            // Grab the part at the clicked cell: store that
                            // cell's offset from the part's center anchor
                            // so it stays under the cursor while dragging.
                            e.held_part = Some(save_view::HeldPart {
                                id: p.id,
                                rot: p.rot,
                                compressed: p.compressed,
                                grab_row: row as i8 - p.row as i8,
                                grab_col: col as i8 - p.col as i8,
                            });
                            // Keep the picker entry in sync so picking is
                            // consistent: the part now shows this rotation
                            // / compression in the palette too.
                            e.part_orient.insert(p.id, (p.rot, p.compressed));
                        }
                        Some(Effect::EditNavicust(NavicustEdit::RemovePart { slot }))
                    }
                    A::ClearNavicust => {
                        if let Some(e) = self.save_view.editing.as_mut() {
                            e.held_part = None;
                        }
                        Some(Effect::EditNavicust(NavicustEdit::ClearAll))
                    }
                    // ----- BN5/BN6 patch-card editor -----
                    A::AddPatchCard56 { id } => Some(Effect::EditPatchCard56s(PatchCard56Edit::AddCard { id })),
                    A::RemovePatchCard56 { slot } => {
                        Some(Effect::EditPatchCard56s(PatchCard56Edit::RemoveCard { slot }))
                    }
                    A::TogglePatchCard56 { slot } => {
                        Some(Effect::EditPatchCard56s(PatchCard56Edit::ToggleCard { slot }))
                    }
                    A::ClearPatchCard56s => Some(Effect::EditPatchCard56s(PatchCard56Edit::ClearAll)),
                    A::ReorderPatchCard56s(ev) => {
                        // Registered list is dense, so any drop is a valid
                        // ordered move; pick-up / cancel are visual-only.
                        use sweeten::widget::drag::DragEvent;
                        match ev {
                            DragEvent::Dropped { index, target_index } if index != target_index => {
                                Some(Effect::EditPatchCard56s(PatchCard56Edit::MoveCard {
                                    from: index,
                                    to: target_index,
                                }))
                            }
                            _ => None,
                        }
                    }
                    // ----- BN4 patch-card editor -----
                    A::AddPatchCard4 { id } => Some(Effect::EditPatchCard4s(PatchCard4Edit::AddCard { id })),
                    A::RemovePatchCard4 { slot } => Some(Effect::EditPatchCard4s(PatchCard4Edit::RemoveCard { slot })),
                    A::TogglePatchCard4 { slot } => Some(Effect::EditPatchCard4s(PatchCard4Edit::ToggleCard { slot })),
                    A::ClearPatchCard4s => Some(Effect::EditPatchCard4s(PatchCard4Edit::ClearAll)),
                    // ----- Auto Battle Data editor -----
                    A::SetChipUseCount { id, count } => {
                        Some(Effect::EditAutoBattleData(AutoBattleDataEdit::SetUseCount {
                            id,
                            count,
                        }))
                    }
                    A::SetSecondaryChipUseCount { id, count } => {
                        Some(Effect::EditAutoBattleData(AutoBattleDataEdit::SetSecondaryUseCount {
                            id,
                            count,
                        }))
                    }
                    A::ClearAutoBattleData => Some(Effect::EditAutoBattleData(AutoBattleDataEdit::ClearAll)),
                    _ => Some(Effect::SaveViewTask(sv_task.map(Message::SaveViewAction))),
                }
            }
            Message::DismissError => {
                self.last_error = None;
                None
            }
            Message::SaveOpenFolder => loadout
                .save
                .as_ref()
                .and_then(|p| p.parent())
                .map(|p| Effect::OpenPath(p.to_path_buf())),
            Message::OpenSavesFolder(path) => Some(Effect::OpenPath(path)),
            Message::SaveDuplicateStart => {
                // Prefill with the next free "<stem> (copy)" name so a
                // plain Enter behaves like the old one-click duplicate.
                let draft = loadout.save.as_deref().map(suggest_duplicate_stem).unwrap_or_default();
                self.save_action = SaveAction::Duplicating { draft };
                None
            }
            Message::SaveDuplicateDraftChanged(s) => {
                if let SaveAction::Duplicating { draft } = &mut self.save_action {
                    *draft = s;
                }
                None
            }
            Message::SaveDuplicateConfirm => {
                let new_stem = if let SaveAction::Duplicating { draft } = &self.save_action {
                    draft.trim().to_string()
                } else {
                    String::new()
                };
                self.save_action = SaveAction::None;
                if new_stem.is_empty() {
                    None
                } else {
                    Some(Effect::SaveDuplicate { new_stem })
                }
            }
            Message::SaveRenameStart => {
                let draft = loadout
                    .save
                    .as_ref()
                    .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                self.save_action = SaveAction::Renaming { draft };
                None
            }
            Message::SaveRenameDraftChanged(s) => {
                if let SaveAction::Renaming { draft } = &mut self.save_action {
                    *draft = s;
                }
                None
            }
            Message::SaveRenameConfirm => {
                let new_stem = if let SaveAction::Renaming { draft } = &self.save_action {
                    draft.trim().to_string()
                } else {
                    String::new()
                };
                self.save_action = SaveAction::None;
                if new_stem.is_empty() {
                    None
                } else {
                    Some(Effect::SaveRename { new_stem })
                }
            }
            Message::SaveDeleteStart => {
                self.save_action = SaveAction::ConfirmDelete;
                None
            }
            Message::SaveDeleteConfirm => {
                self.save_action = SaveAction::None;
                Some(Effect::SaveDelete)
            }
            Message::SaveActionCancel => {
                self.save_action = SaveAction::None;
                None
            }
            Message::SaveNewStart => {
                let saves_dir = config.saves_path();
                // Candidate (variant, template) options span every
                // owned-ROM variant in the family — so you can bootstrap
                // the first save of an empty family, and a dual-ROM owner
                // can pick which color to create. Auto-select only when
                // there's exactly one option; otherwise force an explicit
                // pick (Confirm stays disabled until they do).
                let options = creation_template_options(&config.language, loadout, scanners);
                let (game, template) = if options.len() == 1 {
                    let (game, raw) = options[0].value.clone();
                    (Some(game), Some(raw))
                } else {
                    (None, None)
                };
                let draft = match game {
                    Some(g) => {
                        disambiguate_save_name(&saves_dir, &suggest_save_name(&config.language, g, template.as_deref()))
                    }
                    // No single default yet — seed the field with the
                    // variant-neutral family name so it isn't empty (and
                    // doesn't presume a color) while the user picks a
                    // template.
                    None => loadout
                        .family
                        .map(|f| {
                            disambiguate_save_name(
                                &saves_dir,
                                &sanitize_filename(&game::family_display_name(&config.language, f)),
                            )
                        })
                        .unwrap_or_else(|| "new save".to_string()),
                };
                self.save_action = SaveAction::NewSave {
                    auto_default: Some(draft.clone()),
                    draft,
                    game,
                    template,
                };
                None
            }
            Message::SaveNewDraftChanged(s) => {
                if let SaveAction::NewSave {
                    draft, auto_default, ..
                } = &mut self.save_action
                {
                    if auto_default.as_deref() != Some(s.as_str()) {
                        *auto_default = None;
                    }
                    *draft = s;
                }
                None
            }
            Message::SaveNewTemplateSelected(sel_game, name) => {
                if let SaveAction::NewSave {
                    draft,
                    game,
                    template,
                    auto_default,
                } = &mut self.save_action
                {
                    *game = Some(sel_game);
                    *template = Some(name);
                    if auto_default.as_deref() == Some(draft.as_str()) {
                        let new_draft = disambiguate_save_name(
                            &config.saves_path(),
                            &suggest_save_name(&config.language, sel_game, template.as_deref()),
                        );
                        *draft = new_draft.clone();
                        *auto_default = Some(new_draft);
                    }
                }
                None
            }
            Message::SaveNewConfirm => {
                let SaveAction::NewSave {
                    draft,
                    game: Some(game),
                    template: Some(template),
                    ..
                } = &self.save_action
                else {
                    return None;
                };
                let game = *game;
                let name = draft.trim().to_string();
                let template = template.clone();
                self.save_action = SaveAction::None;
                if name.is_empty() {
                    None
                } else {
                    Some(Effect::SaveNew { name, template, game })
                }
            }
        }
    }
}

impl State {
    #[allow(clippy::too_many_arguments)]
    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loadout: &'a Loadout,
        loaded: Option<&'a selection::Loaded>,
        streamer_mode: bool,
        config: &'a config::Config,
        netplay_phase: &'a crate::netplay::Phase,
        netplay_lobby: &'a crate::netplay::LobbyState,
        netplay_handoff_pending: bool,
        rescanning: bool,
        // Two-phase swap between the bottom bands (link-code strip ↔
        // lobby), driven by the App (which sees the netplay phase
        // flip): first half sinks + dissolves the outgoing band,
        // second half rises the incoming one out of the page surface.
        bottom_swap: &'a crate::anim::Transition,
        // The lobby's last live state, frozen by the App on the
        // frame the band left — the exiting band renders from
        // this so the verdict (e.g. the failure banner) doesn't
        // flash to the idle handshake line mid-dissolve.
        lobby_exit_snapshot: Option<&'a (crate::netplay::Phase, crate::netplay::LobbyState)>,
    ) -> Element<'a, Message> {
        let now = iced::time::Instant::now();
        let mut save_body = self.body(lang, scanners, loadout, loaded, streamer_mode, config, netplay_phase);
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
            self.selector_strip(lang, scanners, loadout, config, rescanning, netplay_handoff_pending),
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
        let (render_lobby, swap) = crate::anim::swap_phase(bottom_swap, now);
        let bottom: Element<'a, Message> = if render_lobby {
            // While the band is on its way OUT, the live phase has
            // already gone Idle (and the lobby may be wiped) — use
            // the snapshot the App froze on the band's last live
            // frame so the verdict doesn't flash mid-dissolve.
            let (band_phase, band_lobby) = if !bottom_swap.shown() {
                lobby_exit_snapshot
                    .map(|(p, l)| (p, l))
                    .unwrap_or((netplay_phase, netplay_lobby))
            } else {
                (netplay_phase, netplay_lobby)
            };
            // Synthesize the local side's Settings from the current
            // loadout so the "You" slot fills in immediately —
            // pre-Lobby phases haven't populated `lobby.local` yet,
            // but everything it needs is already on hand locally.
            // Same builder the netplay loop uses to ship settings on
            // the wire, so the visible info during the handshake
            // exactly matches what gets sent.
            let local_fallback = loadout.make_local_settings(config, netplay_lobby, scanners);
            lobby::Lobby {
                lang,
                state: band_lobby,
                phase: band_phase,
                local_game: loadout.game,
                scanners,
                has_save: loaded.is_some(),
                local_fallback,
                streamer_mode,
                handoff_pending: netplay_handoff_pending,
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
        rescanning: bool,
        inert: bool,
    ) -> Element<'a, Message> {
        // Inert during the PvP handoff window — the loadout pickers
        // reroute to Noop so a mid-spawn selection change can't
        // contradict the committed state, without the strip changing
        // shape.
        let gate = move |m: loadout::Message| if inert { Message::Noop } else { Message::Loadout(m) };
        let game_row: Element<'a, Message> = loadout::game_row(loadout, lang, scanners, config, rescanning).map(gate);
        let save_picker: Element<'a, Message> =
            Element::from(loadout::save_picker(loadout, lang, scanners, config).width(Length::Fill)).map(gate);
        let save_row = self.save_action_row(lang, scanners, loadout, save_picker);

        container(column![game_row, save_row].spacing(6))
            .padding(style::PANE_PADDING)
            .width(Fill)
            .style(widgets::pane)
            .into()
    }

    /// The strip's second row: the save picker + action buttons at
    /// rest, or whichever rename / delete / create-from-template form
    /// is in flight ([`SaveAction`]).
    fn save_action_row<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loadout: &'a Loadout,
        save_picker: Element<'a, Message>,
    ) -> Element<'a, Message> {
        // The picker row fade-through morphs into whichever form
        // opens and back — the swap hides the row's reflow at its
        // fully-dissolved midpoint. While the form is on its way
        // out it has already been reset to `None`, so the exit
        // half renders the frozen copy.
        let now = iced::time::Instant::now();
        let (render_form, form_swap) = crate::anim::swap_phase(&self.save_form, now);
        let action = if render_form && self.save_action == SaveAction::None {
            &self.save_action_exit
        } else {
            &self.save_action
        };
        let mut row_el: Element<'a, Message> =
            self.save_action_row_inner(lang, scanners, loadout, save_picker, render_form, action);
        if let Some(phase) = form_swap {
            row_el = crate::anim::swap_transform(row_el, phase, iced::Vector::new(24.0, 0.0), widgets::plate_color);
        }
        row_el
    }

    fn save_action_row_inner<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loadout: &'a Loadout,
        save_picker: Element<'a, Message>,
        render_form: bool,
        action: &'a SaveAction,
    ) -> Element<'a, Message> {
        if !render_form {
            return row![
                self.new_save_button(lang, scanners, loadout),
                save_picker,
                self.save_action_buttons(lang, loadout),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into();
        }
        match action {
            SaveAction::None => {
                // Form side with nothing recorded (shouldn't happen
                // — the exit snapshot is always set before the swap
                // starts) — degrade to the picker row.
                row![
                    self.new_save_button(lang, scanners, loadout),
                    save_picker,
                    self.save_action_buttons(lang, loadout),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            // Every form ends [× cancel][confirm] — cancel before
            // confirm, same order as the edit-mode Save / Cancel pair
            // and the modal dialogs, so the primary action always
            // sits at the row's end. Confirm buttons repeat the icon
            // of the toolbar action that opened the form, so the form
            // visibly answers the button that started it.
            SaveAction::Renaming { draft } => row![
                text_input(&t!(lang, "save-name-placeholder"), draft)
                    .on_input(Message::SaveRenameDraftChanged)
                    .on_submit(Message::SaveRenameConfirm)
                    .style(widgets::chunky_text_input)
                    .padding(STANDARD_PADDING)
                    .width(Length::Fill),
                widgets::icon_button(
                    Icon::X,
                    t!(lang, "save-action-cancel"),
                    Message::SaveActionCancel,
                    STANDARD_PADDING,
                ),
                widgets::labeled_icon_button(
                    Icon::PencilLine,
                    t!(lang, "save-rename-confirm"),
                    Message::SaveRenameConfirm,
                    STANDARD_PADDING,
                    widgets::primary_button,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            SaveAction::Duplicating { draft } => row![
                text_input(&t!(lang, "save-name-placeholder"), draft)
                    .on_input(Message::SaveDuplicateDraftChanged)
                    .on_submit(Message::SaveDuplicateConfirm)
                    .style(widgets::chunky_text_input)
                    .padding(STANDARD_PADDING)
                    .width(Length::Fill),
                widgets::icon_button(
                    Icon::X,
                    t!(lang, "save-action-cancel"),
                    Message::SaveActionCancel,
                    STANDARD_PADDING,
                ),
                widgets::labeled_icon_button(
                    Icon::Files,
                    t!(lang, "save-duplicate"),
                    Message::SaveDuplicateConfirm,
                    STANDARD_PADDING,
                    widgets::primary_button,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            SaveAction::ConfirmDelete => {
                // Name the target — "Delete BN3 White?" reads as a
                // decision; "Delete this save?" reads as a riddle
                // about what's currently selected.
                let name = loadout
                    .save
                    .as_ref()
                    .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                row![
                    text(t!(lang, "save-delete-prompt", name = name))
                        .style(widgets::muted_text_style)
                        .width(Length::Fill),
                    widgets::icon_button(
                        Icon::X,
                        t!(lang, "save-action-cancel"),
                        Message::SaveActionCancel,
                        STANDARD_PADDING,
                    ),
                    widgets::labeled_icon_button(
                        Icon::Trash,
                        t!(lang, "save-delete-confirm"),
                        Message::SaveDeleteConfirm,
                        STANDARD_PADDING,
                        widgets::danger_button,
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            SaveAction::NewSave {
                draft, game, template, ..
            } => {
                // One option per (owned-ROM variant × template). Each
                // carries the raw name plus a locale-aware label; when the
                // family has more than one owned variant the label is
                // prefixed with the game name ("White – Heat Guts") so the
                // user picks color + template in one go.
                let options = creation_template_options(lang, loadout, scanners);
                let selected = match (game, template) {
                    (Some(g), Some(t)) => options.iter().find(|o| o.value.0 == *g && &o.value.1 == t).cloned(),
                    _ => None,
                };
                let can_confirm = game.is_some() && template.is_some() && !draft.trim().is_empty();
                let confirm_btn = if can_confirm {
                    widgets::labeled_icon_button(
                        Icon::FilePlus,
                        t!(lang, "save-new-confirm"),
                        Message::SaveNewConfirm,
                        STANDARD_PADDING,
                        widgets::primary_button,
                    )
                } else {
                    button(
                        row![Icon::FilePlus.widget(), text(t!(lang, "save-new-confirm"))]
                            .spacing(8)
                            .align_y(Alignment::Center),
                    )
                    .padding(STANDARD_PADDING)
                    .style(widgets::neutral)
                    .into()
                };
                row![
                    pick_list(options, selected, |o: widgets::Choice<(rom::GameRef, String)>| {
                        Message::SaveNewTemplateSelected(o.value.0, o.value.1)
                    })
                    .placeholder(t!(lang, "save-template-pick"))
                    .padding(STANDARD_PADDING)
                    .width(Length::Fixed(180.0))
                    .style(widgets::chunky_pick_list),
                    text_input(&t!(lang, "save-name-placeholder"), draft)
                        .on_input(Message::SaveNewDraftChanged)
                        .on_submit(Message::SaveNewConfirm)
                        .padding(STANDARD_PADDING)
                        .width(Length::Fill)
                        .style(widgets::chunky_text_input),
                    widgets::icon_button(
                        Icon::X,
                        t!(lang, "save-action-cancel"),
                        Message::SaveActionCancel,
                        STANDARD_PADDING,
                    ),
                    confirm_btn,
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
        }
    }

    /// The "new save" button, leading the row from the picker's left —
    /// creation stands apart from the manage-what's-there actions on
    /// the picker's right. Enabled whenever the selected family has an
    /// owned-ROM variant that ships (bundled or patch) save templates
    /// — independent of whether a save is currently selected, so the
    /// first save of an empty family can still be created.
    fn new_save_button<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loadout: &'a Loadout,
    ) -> Element<'a, Message> {
        let can_new = creation_games(loadout, scanners).iter().any(|g| {
            templates_for_game(*g, loadout.patch.as_deref(), loadout.patch_version.as_ref(), scanners).is_some()
        });
        widgets::icon_button_maybe(
            Icon::FilePlus,
            t!(lang, "save-new"),
            can_new.then_some(Message::SaveNewStart),
            STANDARD_PADDING,
        )
    }

    fn save_action_buttons<'a>(&'a self, lang: &'a LanguageIdentifier, loadout: &'a Loadout) -> Element<'a, Message> {
        let enabled = loadout.save.is_some();
        let mk = |icon: Icon, label: String, msg: Message, on: bool| {
            widgets::icon_button_maybe(icon, label, if on { Some(msg) } else { None }, STANDARD_PADDING)
        };
        // Destructive variant for Delete — flags it red so it
        // doesn't look like just another toolbar action.
        let mk_danger = |icon: Icon, label: String, msg: Message, on: bool| {
            widgets::icon_button_styled(
                icon,
                label,
                if on { Some(msg) } else { None },
                STANDARD_PADDING,
                widgets::danger_button,
            )
        };
        row![
            mk(
                Icon::FolderOpen,
                t!(lang, "save-open-folder"),
                Message::SaveOpenFolder,
                enabled
            ),
            mk(
                Icon::Files,
                t!(lang, "save-duplicate"),
                Message::SaveDuplicateStart,
                enabled
            ),
            mk(
                Icon::PencilLine,
                t!(lang, "save-rename"),
                Message::SaveRenameStart,
                enabled
            ),
            mk_danger(Icon::Trash, t!(lang, "save-delete"), Message::SaveDeleteStart, enabled),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
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
    /// (see the FightPressed handler), so the only un-submittable
    /// state is a malformed `/`-command. In streamer mode the input
    /// renders secure (masked) — typed, pasted, and freshly-generated
    /// codes are all scrapeable off a stream otherwise (the generated
    /// one used to flash here in the clear before the lobby's masking
    /// took over).
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

// ---------- New-save template helpers ----------

/// Localized "<game-variant> <template-display>" (or just "<game-variant>"
/// when no template is chosen yet), with filesystem-unsafe characters
/// stripped so it can be dropped straight into the new-save text field.
/// Uses the full variant-aware display name so multi-version games like
/// BN6 Gregar/Falzar get disambiguated.
fn suggest_save_name(lang: &unic_langid::LanguageIdentifier, game: rom::GameRef, template: Option<&str>) -> String {
    let game_name = crate::game::display_name(lang, game);
    let family = game.family_and_variant().0;
    let name = match template {
        Some(raw) => {
            let label = template_label(lang, family, raw);
            format!("{game_name} - {label}")
        }
        None => game_name,
    };
    sanitize_filename(&name)
}

fn sanitize_filename(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if (c as u32) < 0x20 => ' ',
            c => c,
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Appends ` 2`, ` 3`, ... to `base` until the resulting `<name>.sav`
/// doesn't already exist in `saves_dir`. Gives up at 99 to avoid an
/// unbounded scan if the directory is somehow saturated.
fn disambiguate_save_name(saves_dir: &std::path::Path, base: &str) -> String {
    let mut draft = base.to_string();
    for n in 2..100 {
        if !saves_dir.join(format!("{draft}.sav")).exists() {
            break;
        }
        draft = format!("{base} {n}");
    }
    draft
}

/// Owned-ROM games in the selected family, ascending variant order —
/// the candidate targets for creating a new save. Empty when no family
/// is selected or no ROM is owned. Independent of the resolved game, so
/// the new-save flow works even before any save exists in the family.
/// When a patch is selected, variants it doesn't support are dropped
/// (so their templates don't show) — creating a save under an active
/// patch is a patch-specific flow.
fn creation_games(loadout: &Loadout, scanners: &Scanners) -> Vec<rom::GameRef> {
    let Some(family) = loadout.family else {
        return Vec::new();
    };
    let roms = scanners.roms.read();
    let patch_supported = loadout::patch_supported_games(loadout, scanners);
    game::games_in_family(family)
        .filter(|g| roms.contains_key(g))
        .filter(|g| patch_supported.as_ref().map(|s| s.contains(g)).unwrap_or(true))
        .collect()
}

/// Save templates for one specific game (patch-provided override the
/// bundled ones), keyed by template name (empty string = default).
/// None when that game ships no templates.
fn templates_for_game(
    game: rom::GameRef,
    patch_name: Option<&str>,
    patch_version: Option<&semver::Version>,
    scanners: &Scanners,
) -> Option<indexmap::IndexMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>> {
    // IndexMap (not BTreeMap) so templates iterate in declaration order
    // — patch-provided first, then the game's bundled order — instead
    // of alphabetically by raw key.
    let mut out = indexmap::IndexMap::new();
    if let (Some(patch_name), Some(version)) = (patch_name, patch_version) {
        let patches = scanners.patches.read();
        if let Some(patch) = patches.get(patch_name) {
            if let Some(v) = patch.versions.get(version) {
                if let Some(m) = v.save_templates.get(&game) {
                    for (name, save) in m.iter() {
                        out.insert(name.clone(), save.clone_box());
                    }
                }
            }
        }
    }
    // Fall back to bundled per-game templates registered via the Game
    // trait. Patch templates take precedence: if a patch ships a
    // "heat-guts" template, it overrides the built-in of the same name.
    if let Some(game_impl) = game::from_gamedb_entry(game) {
        for (name, save) in game_impl.save_templates.iter() {
            out.entry((*name).to_string()).or_insert_with(|| save.clone_box());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Picker entries for the new-save dialog: every (owned-ROM variant ×
/// template) across the selected family. Each label is prefixed with the
/// short variant tag (e.g. "Blue – Heat Guts") in all cases.
fn creation_template_options(
    lang: &unic_langid::LanguageIdentifier,
    loadout: &Loadout,
    scanners: &Scanners,
) -> Vec<widgets::Choice<(rom::GameRef, String)>> {
    let games = creation_games(loadout, scanners);
    let mut out = Vec::new();
    for g in games {
        if let Some(tmpls) = templates_for_game(g, loadout.patch.as_deref(), loadout.patch_version.as_ref(), scanners) {
            for name in tmpls.keys() {
                out.push(save_template_choice(lang, g, name));
            }
        }
    }
    out
}

/// Resolve the actual template `Save` for a (game, template-name) pick —
/// used by the App's SaveNew handler to materialize the file. Falls back
/// to the default/first template if the exact name vanished.
pub fn creation_template(
    game: rom::GameRef,
    template_name: &str,
    loadout: &Loadout,
    scanners: &Scanners,
) -> Option<Box<dyn tango_dataview::save::Save + Send + Sync>> {
    let tmpls = templates_for_game(game, loadout.patch.as_deref(), loadout.patch_version.as_ref(), scanners)?;
    tmpls
        .get(template_name)
        .or_else(|| tmpls.get(""))
        .or_else(|| tmpls.values().next())
        .map(|s| s.clone_box())
}

/// Bare localized template label (e.g. "Heat Guts"), without any
/// variant prefix. Empty `raw` is the unnamed default-template file that
/// patches ship as `<rom>_<rev>.sav`; the `.save-megaman` attr usually
/// carries the right label for it.
fn template_label(lang: &unic_langid::LanguageIdentifier, family: &str, raw: &str) -> String {
    let key_suffix = if raw.is_empty() { "megaman" } else { raw };
    // Dynamic key (one per family × template name) — bypass the
    // literal-only macro and hit the Fluent loader directly.
    use fluent_templates::Loader;
    crate::i18n::LOCALES
        .try_lookup(lang, &format!("game-{family}.save-{key_suffix}"))
        .unwrap_or_else(|| {
            if raw.is_empty() {
                t!(lang, "save-template-default")
            } else {
                raw.to_string()
            }
        })
}

/// One entry in the "new save" template pick_list: a concrete variant
/// plus a raw template name, with a display label resolved via
/// `game-<family>.save-<name>` (prefixed with the game name when the
/// family has more than one owned variant). The value is `(variant,
/// raw template name)` — the two together pick a unique creation
/// target.
fn save_template_choice(
    lang: &unic_langid::LanguageIdentifier,
    game: rom::GameRef,
    raw: &str,
) -> widgets::Choice<(rom::GameRef, String)> {
    let label = template_label(lang, game.family_and_variant().0, raw);
    // Always prefix with the short variant tag (e.g. "Blue – Heat
    // Guts"), even for single-owned-variant or single-variant
    // families, so the picker reads consistently.
    let display = format!("{} \u{2013} {}", crate::game::variant_short_name(lang, game), label);
    widgets::Choice::new((game, raw.to_string()), display)
}

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

/// Next free "<stem> (copy)" / "<stem> (copy N)" stem for `src` —
/// the prefill for the duplicate form, so a plain Enter behaves like
/// the old one-click duplicate.
fn suggest_duplicate_stem(src: &std::path::Path) -> String {
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = src.extension().map(|e| e.to_string_lossy().into_owned());
    for n in 1..1000 {
        let suffix = if n == 1 {
            " (copy)".to_string()
        } else {
            format!(" (copy {n})")
        };
        let candidate_stem = format!("{stem}{suffix}");
        let filename = match &ext {
            Some(ext) => format!("{candidate_stem}.{ext}"),
            None => candidate_stem.clone(),
        };
        let taken = src.parent().map(|p| p.join(filename).exists()).unwrap_or(false);
        if !taken {
            return candidate_stem;
        }
    }
    format!("{stem} (copy)")
}

/// Copy `src` to a sibling file named `new_stem` (extension
/// preserved). Refuses path-traversal, empty names, and existing
/// destinations — same rules as [`rename_save`].
pub fn duplicate_save(src: &std::path::Path, new_stem: &str) -> anyhow::Result<std::path::PathBuf> {
    if new_stem.is_empty() {
        anyhow::bail!("empty save name");
    }
    if new_stem.contains('/') || new_stem.contains('\\') || new_stem.contains("..") {
        anyhow::bail!("invalid save name");
    }
    let parent = src.parent().ok_or_else(|| anyhow::anyhow!("save has no parent dir"))?;
    let ext = src.extension().map(|e| e.to_string_lossy().into_owned());
    let new_name = if let Some(ext) = ext {
        format!("{new_stem}.{ext}")
    } else {
        new_stem.to_string()
    };
    let dst = parent.join(new_name);
    if dst == src || dst.exists() {
        anyhow::bail!("destination already exists");
    }
    std::fs::copy(src, &dst)?;
    Ok(dst)
}

/// Rename `src` to use `new_stem` (extension preserved). Refuses
/// path-traversal or empty names.
pub fn rename_save(src: &std::path::Path, new_stem: &str) -> anyhow::Result<std::path::PathBuf> {
    if new_stem.is_empty() {
        anyhow::bail!("empty save name");
    }
    if new_stem.contains('/') || new_stem.contains('\\') || new_stem.contains("..") {
        anyhow::bail!("invalid save name");
    }
    let parent = src.parent().ok_or_else(|| anyhow::anyhow!("save has no parent dir"))?;
    let ext = src.extension().map(|e| e.to_string_lossy().into_owned());
    let new_name = if let Some(ext) = ext {
        format!("{new_stem}.{ext}")
    } else {
        new_stem.to_string()
    };
    let dst = parent.join(new_name);
    if dst == src {
        return Ok(dst);
    }
    if dst.exists() {
        anyhow::bail!("destination already exists");
    }
    std::fs::rename(src, &dst)?;
    Ok(dst)
}

/// Write a template's SRAM to `saves_dir/<name>.sav`. The filename is
/// taken verbatim from `name` (trimmed); on collisions returns Err.
///
/// `rebuild_checksum()` is required before `to_sram_dump()` — without
/// it the SRAM checksum is stale (computed at template-construction
/// time, before this game-specific clone) and both the GBA game and
/// Tango's `parse_save` reject the resulting file. The legacy app
/// does the same in `gui/save_select_view.rs::create_new_save`.
pub fn create_new_save(
    saves_dir: &std::path::Path,
    name: &str,
    template: &dyn tango_dataview::save::Save,
) -> anyhow::Result<std::path::PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("empty save name");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        anyhow::bail!("invalid save name");
    }
    let filename = if name.ends_with(".sav") {
        name.to_string()
    } else {
        format!("{name}.sav")
    };
    let dst = saves_dir.join(filename);
    if dst.exists() {
        anyhow::bail!("destination already exists");
    }
    std::fs::create_dir_all(saves_dir)?;
    let mut save = template.clone_box();
    save.rebuild_checksum();
    let sram = save.to_sram_dump();
    std::fs::write(&dst, sram)?;
    Ok(dst)
}

// ---------- "Commit to a match" CTA chrome ----------
//
// Shared between the bottom strip's Fight button and the lobby's
// Ready toggle — both are the same "slam this to fight" affordance,
// so they wear the same chrome.

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
/// * Idle      — primary_button on steroids: brighter gradient,
///               huge primary glow, chunky 2 px border. This is
///               the moment the user is supposed to slam the
///               button, so it has to feel hot.
/// * Committed — neutral beveled plate. We've ack'd locally and
///               are waiting on the peer; the only useful action
///               is to take it back, which is not a celebration.
/// * Starting  — flat muted badge. Both sides committed; the
///               button is now purely a status indicator with no
///               click target.
fn ready_button_style(theme: &iced::Theme, status: button::Status, palette: ReadyPalette) -> button::Style {
    let p = theme.extended_palette();
    let primary = theme.palette().primary;
    match palette {
        ReadyPalette::Starting => button::Style {
            background: Some(iced::Background::Color(p.background.weak.color)),
            text_color: widgets::muted_color(theme),
            border: iced::Border {
                radius: 10.0.into(),
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
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    radius: 10.0.into(),
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
/// - `/host`            — listen on [`crate::net::DEFAULT_LOCAL_PORT`]
/// - `/host <port>`     — listen on the given port
/// - `/connect <addr>`  — dial `<addr>`, appending the default
///                        port if the user didn't specify one
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
