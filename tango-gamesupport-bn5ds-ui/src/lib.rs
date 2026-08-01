//! BN5DS's save-editor UI: the chip folder as a read-only viewer, the
//! GBA-slot cross in the identity slot BN5/BN6 name their navi in, and —
//! while editing — a switcher for which of the cartridge's two in-game
//! files is the one being played.
//!
//! The cross is what this save brings to a battle, so it reads as a name
//! above the tab body on every view and turns into its own dropdown when
//! the edit session opens; a save with none brings plain MegaMan, which
//! is a name like any other. Under the name sits which of the
//! cartridge's two teams the file plays — the cartridge's other half of
//! who the player fields, and not a pick: the game asks for it once, at
//! the file's first boot, and the save carries the answer from then on.
//! The file pick is only there while editing.
//!
//! Chip editing is not plumbed here (the save hands out no writable
//! chips view — see its `view_chips_mut`), so the tab body is the
//! ordinary viewer whether or not an edit session is open. The two picks
//! are the whole edit session: both write the cartridge's own bytes,
//! both are staged like any other edit and land on Save.
//!
//! Why they are edits rather than view state: nothing rides beside a
//! committed cartridge any more. A file becomes the played one by
//! becoming the one the game itself calls most recently saved
//! (`Save::make_current`), and a cross is the byte the game's own file
//! select writes — so a peer's priming, a recording's playback and this
//! view all read the same answer out of the same bytes.

use std::sync::Arc;

use sweeten::widget::column;
use tango_gamesupport_bn5ds_dataview::save::{Cross, Save, SaveSet};
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

/// Play the cartridge's other in-game file: point the editor at it, and
/// stamp it as the cartridge's most recently saved file so everything
/// that reads these bytes — a session, a recording, the priming walk's
/// file select — lands on it too.
///
/// Re-reads the set from the dump the loaded save carries, staged edits
/// included, since they live in those bytes.
#[derive(Debug)]
struct PlayFile(u8);

impl GameEdit for PlayFile {
    fn apply(&self, model: &mut tango_gamesupport_common::model::SaveModel) -> Invalidation {
        let Some(mut next) = model
            .save
            .as_any()
            .downcast_ref::<Save>()
            .and_then(|save| SaveSet::parse(&save.to_sram_dump()).ok())
            .and_then(|set| set.save(self.0))
        else {
            return Invalidation::default();
        };
        next.make_current();
        model.save = Box::new(next);
        // A different file can differ in what it offers to edit, so the
        // cached capability flags are re-probed against the new save.
        tango_gamesupport_common::model::refresh_editability(model);
        Invalidation::default()
    }
}

/// Set the cross the played file brings.
#[derive(Debug)]
struct SetCross(Cross);

impl GameEdit for SetCross {
    fn apply(&self, model: &mut tango_gamesupport_common::model::SaveModel) -> Invalidation {
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            save.set_cross(self.0);
        }
        Invalidation::default()
    }
}

/// One of the cartridge's files paired with its localized label, for the
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

/// One cross the player may bring, labeled. The two BassCross values
/// are one choice here: which of them a pick lands is the save's team's
/// to say, exactly as the game decides it.
#[derive(Clone, PartialEq)]
struct CrossChoice {
    cross: Cross,
    label: String,
}

impl std::fmt::Display for CrossChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

fn pick_list<'a, T, M>(options: Vec<T>, selected: Option<T>, on_select: impl Fn(T) -> M + 'a) -> iced::Element<'a, M>
where
    T: ToString + PartialEq + Clone + 'a,
    M: Clone + 'a,
{
    sweeten::widget::pick_list(options, selected, on_select)
        .padding(tango_gamesupport_common::style::CONTROL_PADDING)
        .text_size(tango_gamesupport_common::style::TEXT_BODY)
        .style(tango_gamesupport_common::widgets::chunky_pick_list)
        .into()
}

/// The file switcher: a dropdown over the cartridge's file-select slots,
/// naming them the way the game's own file select does. `None` when
/// there is only one file (a damaged dump) — then there is nothing to
/// switch.
fn file_picker<'a>(lang: &'a LanguageIdentifier, save: &'a Save) -> Option<iced::Element<'a, Action>> {
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
    Some(pick_list(options, selected, |c: FileChoice| {
        Action::Game(Arc::new(PlayFile(c.slot)))
    }))
}

/// What the save brings, as the choices offered for it: plain MegaMan,
/// or one of the two the game unlocks from a cartridge in the DS's GBA
/// slot. BassCross is one entry rather than two — which of the game's
/// two values a pick lands is the save's team's to say, exactly as the
/// game decides it.
///
/// The second half is which of them the save is on. A save carrying the
/// *other* team's BassCross (an editor pass from a cartridge whose team
/// differs, or a real slot-2 boot) still reads as BassCross rather than
/// as nothing.
fn cross_choices<'a>(lang: &'a LanguageIdentifier, save: &'a Save) -> (Vec<CrossChoice>, Option<CrossChoice>) {
    let options: Vec<CrossChoice> = [
        (Cross::None, tango_gamesupport_common::t!(lang, "bn5ds-cross-none")),
        (
            Cross::bass_for(save.team()),
            tango_gamesupport_common::t!(lang, "bn5ds-cross-bass"),
        ),
        (Cross::Sol, tango_gamesupport_common::t!(lang, "bn5ds-cross-sol")),
    ]
    .into_iter()
    .map(|(cross, label)| CrossChoice { cross, label })
    .collect();
    let current = save.cross();
    let selected = options
        .iter()
        .find(|c| c.cross == current || (c.cross.is_bass() && current.is_bass()))
        .cloned();
    (options, selected)
}

/// How tall the naming line is on both halves of the card: the picker's
/// own height, derived the way the tab strip derives its own rather than
/// measured — its text at iced's default 1.3 line height, plus
/// [`CONTROL_PADDING`] top and bottom and the pick list's 1px border on
/// each side.
///
/// The reading half is pinned to it so opening the edit session doesn't
/// change the strip's height and shove the whole tab body down. Its own
/// name sits at [`TEXT_TITLE`], which is shorter, so without this the
/// swap would move everything below by a few pixels.
///
/// [`CONTROL_PADDING`]: tango_gamesupport_common::style::CONTROL_PADDING
/// [`TEXT_TITLE`]: tango_gamesupport_common::style::TEXT_TITLE
const CARD_HEIGHT: f32 = tango_gamesupport_common::style::TEXT_BODY * 1.3
    + tango_gamesupport_common::style::CONTROL_PADDING[0] * 2.0
    + 2.0;

/// Which of the cartridge's two teams this file plays — the fact the
/// game itself asks for once and then never asks again, and the one that
/// decides which of the two BassCross values a pick lands (see
/// [`Cross::bass_for`]). Read out of the file's own bytes, so it names
/// the file being looked at rather than the cartridge.
fn team_label(lang: &LanguageIdentifier, save: &Save) -> String {
    if save.team() == 0 {
        tango_gamesupport_common::t!(lang, "bn5ds-team-protoman")
    } else {
        tango_gamesupport_common::t!(lang, "bn5ds-team-colonel")
    }
}

/// One half of the identity card, in the box both halves share: the
/// naming line — a name while reading, its dropdown while editing —
/// over the file's team.
///
/// The team line is on both halves, and the naming line is pinned to
/// [`CARD_HEIGHT`], so opening the edit session swaps one line for the
/// other without moving anything below it.
///
/// Carries the strip's own left inset, which the strip expects a card to
/// bring (see `render_identity_strip`).
fn card_slot<'a>(inner: iced::Element<'a, Action>, team: String) -> iced::Element<'a, Action> {
    iced::widget::container(
        column![
            iced::widget::container(inner)
                .height(iced::Length::Fixed(CARD_HEIGHT))
                .align_y(iced::Alignment::Center),
            iced::widget::text(team)
                .size(tango_gamesupport_common::style::TEXT_CAPTION)
                .style(tango_gamesupport_common::widgets::muted_text_style)
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(4),
    )
    .padding([4.0, 6.0])
    .into()
}

/// The identity card: the name of what this save brings, in the slot
/// BN5/BN6 name their navi in, over the team the file plays. Every save
/// shows one — a save with no cross brings plain MegaMan, which is a
/// name like any other.
fn cross_card<'a>(lang: &'a LanguageIdentifier, save: &'a Save) -> iced::Element<'a, Action> {
    let (options, selected) = cross_choices(lang, save);
    let name = selected.map(|c| c.label).unwrap_or_else(|| {
        options
            .first()
            .map(|c| c.label.clone())
            .unwrap_or_else(|| tango_gamesupport_common::t!(lang, "bn5ds-cross-none"))
    });
    card_slot(
        iced::widget::text(name)
            .size(tango_gamesupport_common::style::TEXT_TITLE)
            .wrapping(iced::widget::text::Wrapping::None)
            .into(),
        team_label(lang, save),
    )
}

/// The same card while editing: the dropdown that changes the name. The
/// team below it stays a reading — it is the file's, and the file select
/// is what moves between them.
fn cross_picker<'a>(lang: &'a LanguageIdentifier, save: &'a Save) -> iced::Element<'a, Action> {
    let (options, selected) = cross_choices(lang, save);
    card_slot(
        pick_list(options, selected, |c: CrossChoice| {
            Action::Game(Arc::new(SetCross(c.cross)))
        }),
        team_label(lang, save),
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

    /// Which of the cartridge's files is the played one. Cartridge-wide
    /// rather than any one section's, which is what puts it in the bar
    /// instead of a tab body — and an edit like any other, so it is only
    /// there while the session is open.
    fn top_bar_control<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        loaded: &'a OpenSave,
    ) -> Option<iced::Element<'a, Action>> {
        file_picker(lang, file_of(loaded)?)
    }

    /// The shared strip around this cartridge's own card: what the save
    /// brings to a battle, named while reading and picked while editing.
    /// This cartridge has no navi roster for the default card to show,
    /// and the cross is the same kind of fact — who the player fields —
    /// so it takes the slot the other games name their navi in.
    fn identity_strip<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        loaded: &'a OpenSave,
        edit: Option<Action>,
        editing: bool,
        actions: iced::Element<'a, Action>,
    ) -> iced::Element<'a, Action> {
        let Some(save) = file_of(loaded) else {
            return sv::navi::render_navi_strip(lang, loaded, edit, actions);
        };
        let card = if editing {
            cross_picker(lang, save)
        } else {
            cross_card(lang, save)
        };
        sv::navi::render_identity_strip(card, actions)
    }

    /// There is always something to edit on this cartridge: the cross
    /// pick, which every save carries. That is what puts the Edit button
    /// up while the chip editor is unplumbed.
    fn extra_editable(&self, loaded: &OpenSave) -> bool {
        file_of(loaded).is_some()
    }

    /// The folder-full rule guards the chip editor, which isn't plumbed
    /// here — the bar's picks are always committable, and a save whose
    /// folder happens to have a gap must not be barred from saving one.
    fn can_save(&self, _loaded: &OpenSave) -> bool {
        true
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

    /// Never reached: no section of this save is editable (`tab_editable`
    /// answers the shared capability probe, and the chips view hands out
    /// no writable half), so an open edit session keeps showing the
    /// read-only body while the bar carries the picks.
    fn render_edit<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        _tab: Tab,
        _loaded: &'a OpenSave,
        _state: &'a State,
    ) -> iced::Element<'a, Action> {
        sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty"))
    }

    fn tab_as_text(&self, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
        match tab {
            Tab::Folder => sv::folder::as_text(loaded, opts),
            _ => None,
        }
    }
}
