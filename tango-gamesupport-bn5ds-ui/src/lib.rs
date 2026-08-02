//! BN5DS's save-editor UI: the NaviCust, the chip folder, the
//! auto-battle data, the GBA-slot cross in the identity slot BN5/BN6
//! name their navi in, and — while editing — a switcher for which of
//! the cartridge's two in-game files is the one being played.
//!
//! The tabs are BN5's minus the patch cards, which Double Team dropped;
//! each is the shared editor, unadorned, since the save exposes the
//! same views the GBA game's does.
//!
//! The cross is what this save brings to a battle, so it reads as a name
//! above the tab body on every view and turns into its own dropdown when
//! the edit session opens; a save with none brings plain MegaMan, which
//! is a name like any other. Under the name sits which of the
//! cartridge's two teams the file plays — the cartridge's other half of
//! who the player fields, and not a pick: the game asks for it once, at
//! the file's first boot, and the save carries the answer from then on —
//! then the HP MegaMan brings, and under both the two navis the battle's
//! own NAVI CHANGE panel offers, which *are* a pick. The file pick is
//! only there while editing.
//!
//!
//! Why they are edits rather than view state: nothing rides beside a
//! committed cartridge any more. A file becomes the played one by
//! becoming the one the game itself calls most recently saved
//! (`Save::make_current`), and a cross is the byte the game's own file
//! select writes — so a peer's priming, a recording's playback and this
//! view all read the same answer out of the same bytes.

use std::sync::Arc;

use sweeten::widget::{column, row};
use tango_gamesupport_bn5ds_dataview::rom;
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

/// The cart's own assets, through any override layering — what names
/// the crosses. `underlying_any`, not `as_any`: a patch's name
/// overrides reach chips, not this cartridge's own MegaMen.
fn cart_of(loaded: &OpenSave) -> Option<&rom::Assets> {
    loaded.assets.underlying_any().downcast_ref::<rom::Assets>()
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

/// Put a navi in one of the battle's two NAVI CHANGE slots, or empty
/// it.
///
/// A navi can only be in one slot, so picking one that is already in
/// the other slot trades places with it rather than cloning it — which
/// is also the shortest way to reorder the pair.
#[derive(Debug)]
struct SetTeamNavi {
    slot: usize,
    navi: Option<usize>,
}

impl GameEdit for SetTeamNavi {
    fn apply(&self, model: &mut tango_gamesupport_common::model::SaveModel) -> Invalidation {
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            let held = save.team_navi(self.slot);
            for other in 0..tango_gamesupport_bn5ds_dataview::save::NUM_TEAM_SLOTS {
                if other != self.slot && self.navi.is_some() && save.team_navi(other) == self.navi {
                    save.set_team_navi(other, held);
                }
            }
            save.set_team_navi(self.slot, self.navi);
            // The team is a packed list — a gap is a team the battle
            // refuses. See `Save::pack_team`.
            save.pack_team();
        }
        Invalidation::default()
    }
}

/// One navi a team slot may hold — or the empty pick — labeled with the
/// cart's own name for it.
#[derive(Clone, PartialEq)]
struct NaviChoice {
    navi: Option<usize>,
    label: String,
}

impl std::fmt::Display for NaviChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
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
///
/// The labels are the cart's own names for these three — `MegaMan`,
/// `BCMegaMn`, `SCMegaMn` and their JP counterparts — so they read the
/// way the game itself writes them rather than the way English prose
/// would. A cart that won't give one up (a patch that moved the
/// archives) falls back to naming the byte, which is unpickable
/// otherwise.
fn cross_choices<'a>(loaded: &'a OpenSave, save: &'a Save) -> (Vec<CrossChoice>, Option<CrossChoice>) {
    let cart = cart_of(loaded);
    let options: Vec<CrossChoice> = [Cross::None, Cross::bass_for(save.team()), Cross::Sol]
        .into_iter()
        .map(|cross| CrossChoice {
            cross,
            label: cart
                .and_then(|cart| cart.cross_name(cross))
                .unwrap_or_else(|| format!("Cross #{}", cross.raw())),
        })
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
///
/// Named by its leader, as the cart's own file select names it: the
/// navi is the cart's word, the label beside it this app's.
fn team_stat<'a>(lang: &LanguageIdentifier, loaded: &OpenSave, save: &Save) -> iced::Element<'a, Action> {
    let leader = cart_of(loaded)
        .and_then(|cart| cart.leader_name(save.team()))
        .unwrap_or_else(|| format!("#{}", save.team()));
    sv::stat(tango_gamesupport_common::t!(lang, "bn5ds-leader"), leader)
}

/// One navi as the dropdown shows it, named by the cart — a navi it
/// won't name reads as its number, which is still pickable.
fn navi_choice(lang: &LanguageIdentifier, loaded: &OpenSave, navi: Option<usize>) -> NaviChoice {
    NaviChoice {
        navi,
        label: match navi {
            None => tango_gamesupport_common::t!(lang, "bn5ds-team-none"),
            Some(navi) => cart_of(loaded)
                .and_then(|cart| cart.navi_name(navi))
                .unwrap_or_else(|| format!("#{navi}")),
        },
    }
}

/// What a team slot may hold: any of the twelve navis, or the empty
/// pick. The save layer keeps the mirror bits the game's load checks a
/// team against in step with every write (see `Save::set_team_navi`),
/// which is what makes the full roster — either team's half — safe to
/// offer.
fn navi_choices(lang: &LanguageIdentifier, loaded: &OpenSave, save: &Save) -> Vec<NaviChoice> {
    std::iter::once(None)
        .chain(save.team_navi_choices().into_iter().map(Some))
        .map(|navi| navi_choice(lang, loaded, navi))
        .collect()
}

/// The team the save brings into a battle: the two navis its NAVI
/// CHANGE panel offers, in the panel's own order — a reading while the
/// card is, and a pair of dropdowns while the edit session is open.
///
/// The dropdowns offer the whole roster for each slot, plus the empty
/// pick; a navi picked into both slots trades places instead of
/// duplicating, and every edit re-packs the pair (the game keeps the
/// team as a packed list and refuses a gap).
fn team_line<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a OpenSave,
    save: &'a Save,
    editing: bool,
) -> Option<iced::Element<'a, Action>> {
    let choices = navi_choices(lang, loaded, save);
    let mut line = row![].spacing(8).align_y(iced::Alignment::End);
    for slot in 0..tango_gamesupport_bn5ds_dataview::save::NUM_TEAM_SLOTS {
        // Named from the slot itself rather than looked up among the
        // choices, so a navi the picker wouldn't offer still reads as
        // the navi it is.
        let held = navi_choice(lang, loaded, save.team_navi(slot));
        line = line.push(if editing {
            pick_list(choices.clone(), Some(held), move |choice: NaviChoice| {
                Action::Game(Arc::new(SetTeamNavi {
                    slot,
                    navi: choice.navi,
                }))
            })
        } else {
            // The label alone: the same words the dropdown would show.
            iced::widget::text(held.label)
                .size(tango_gamesupport_common::style::TEXT_BODY)
                .wrapping(iced::widget::text::Wrapping::None)
                .into()
        });
    }
    Some(
        row![
            iced::widget::text(tango_gamesupport_common::t!(lang, "bn5ds-team"))
                .size(tango_gamesupport_common::style::TEXT_CAPTION)
                .style(tango_gamesupport_common::widgets::muted_text_style)
                .wrapping(iced::widget::text::Wrapping::None),
            line,
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center)
        .into(),
    )
}

/// The HP MegaMan brings. The figure is the save's own — HP Memories
/// plus what the NaviCust adds — so it moves as the NaviCust tab is
/// edited.
fn hp_stat<'a>(lang: &'a LanguageIdentifier, loaded: &'a OpenSave) -> Option<iced::Element<'a, Action>> {
    let hp = loaded.save.view_navi()?.max_hp(loaded.assets.as_ref());
    Some(sv::stat(
        tango_gamesupport_common::t!(lang, "navi-base-hp"),
        hp.to_string(),
    ))
}

/// One half of the identity card, in the box both halves share: the
/// naming line — a name while reading, its dropdown while editing —
/// over the file's team and the HP it brings.
///
/// The caption line is on both halves, and the naming line is pinned to
/// [`CARD_HEIGHT`], so opening the edit session swaps one line for the
/// other without moving anything below it.
///
/// Carries the strip's own left inset, which the strip expects a card to
/// bring (see `render_identity_strip`).
fn card_slot<'a>(
    inner: iced::Element<'a, Action>,
    team: iced::Element<'a, Action>,
    hp: Option<iced::Element<'a, Action>>,
    navis: Option<iced::Element<'a, Action>>,
) -> iced::Element<'a, Action> {
    let mut stats = row![team].spacing(16).align_y(iced::Alignment::End);
    if let Some(hp) = hp {
        stats = stats.push(hp);
    }
    let mut card = column![
        iced::widget::container(inner)
            .height(iced::Length::Fixed(CARD_HEIGHT))
            .align_y(iced::Alignment::Center),
        stats,
    ]
    .spacing(4);
    if let Some(navis) = navis {
        card = card.push(navis);
    }
    iced::widget::container(card).padding([4.0, 6.0]).into()
}

/// The identity card: the name of what this save brings, in the slot
/// BN5/BN6 name their navi in, over the team the file plays. Every save
/// shows one — a save with no cross brings plain MegaMan, which is a
/// name like any other.
fn cross_card<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a OpenSave,
    save: &'a Save,
) -> iced::Element<'a, Action> {
    let (options, selected) = cross_choices(loaded, save);
    let name = selected
        .or_else(|| options.first().cloned())
        .map(|c| c.label)
        .unwrap_or_default();
    card_slot(
        iced::widget::text(name)
            .size(tango_gamesupport_common::style::TEXT_TITLE)
            .wrapping(iced::widget::text::Wrapping::None)
            .into(),
        team_stat(lang, loaded, save),
        hp_stat(lang, loaded),
        team_line(lang, loaded, save, false),
    )
}

/// The same card while editing: the dropdown that changes the name. The
/// team below it stays a reading — it is the file's, and the file select
/// is what moves between them.
fn cross_picker<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a OpenSave,
    save: &'a Save,
) -> iced::Element<'a, Action> {
    let (options, selected) = cross_choices(loaded, save);
    card_slot(
        pick_list(options, selected, |c: CrossChoice| {
            Action::Game(Arc::new(SetCross(c.cross)))
        }),
        team_stat(lang, loaded, save),
        hp_stat(lang, loaded),
        team_line(lang, loaded, save, true),
    )
}

impl GameSaveEditor for Ui {
    /// BN5's tabs, minus the patch cards this cart has none of. The
    /// NaviCust drops out for a file being played as a team navi, the
    /// way the GBA game's does for a link navi.
    fn tabs(&self, loaded: &OpenSave) -> Vec<Tab> {
        let save = loaded.save.as_ref();
        let mut tabs = vec![];
        if save.view_navicust().is_some() {
            tabs.push(Tab::Navicust);
        }
        if save.view_chips().is_some() {
            tabs.push(Tab::Folder);
        }
        if save.view_auto_battle_data().is_some() {
            tabs.push(Tab::AutoBattleData);
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
            cross_picker(lang, loaded, save)
        } else {
            cross_card(lang, loaded, save)
        };
        sv::navi::render_identity_strip(card, actions)
    }

    /// There is always something to edit on this cartridge: the cross
    /// pick, which every save carries. That is what puts the Edit button
    /// up while the chip editor is unplumbed.
    fn extra_editable(&self, loaded: &OpenSave) -> bool {
        file_of(loaded).is_some()
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
            Tab::Navicust => sv::navicust::render_navicust_tab(lang, loaded),
            Tab::Folder => sv::folder::render_folder(lang, loaded, opts.folder_grouped),
            Tab::AutoBattleData => sv::abd::render_auto_battle_data(lang, loaded),
            _ => sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty")),
        }
    }

    fn render_edit<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        state: &'a State,
    ) -> iced::Element<'a, Action> {
        match tab {
            Tab::Navicust => sv::navicust::render_navicust_edit(lang, loaded, state),
            Tab::Folder => sv::folder::render_folder_edit(lang, loaded, state),
            Tab::AutoBattleData => sv::abd::render_auto_battle_data_edit(lang, loaded, state),
            _ => sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty")),
        }
    }

    fn tab_as_text(&self, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
        match tab {
            Tab::Navicust => sv::navicust::navicust_as_text(loaded),
            Tab::Folder => sv::folder::as_text(loaded, opts),
            Tab::AutoBattleData => sv::abd::as_text(loaded),
            _ => None,
        }
    }

    fn tab_as_image(&self, tab: Tab, loaded: &OpenSave) -> Option<image::RgbaImage> {
        match tab {
            Tab::Navicust => sv::navicust::as_image(loaded),
            _ => None,
        }
    }
}
