//! BN5DS's save-editor UI: the NaviCust, the chip folder, the
//! auto-battle data, the GBA-slot cross in the identity slot BN5/BN6
//! name their navi in, and — while editing — a switcher for which of
//! the cartridge's two in-game files is the one being played.
//!
//! The tabs are BN5's minus the patch cards, which Double Team dropped,
//! plus the Party tab for the game's own battle-team pair; the shared
//! sections are the shared editor, unadorned, since the save exposes
//! the same views the GBA game's does.
//!
//! The cross is what this save brings to a battle, so it reads as a name
//! above the tab body on every view and turns into its own dropdown when
//! the edit session opens; a save with none brings plain MegaMan, which
//! is a name like any other. Under the name sits which of the
//! cartridge's two teams the file plays — the cartridge's other half of
//! who the player fields, and not a pick: the game asks for it once, at
//! the file's first boot, and the save carries the answer from then on —
//! then the HP MegaMan brings. The party — the two navis the battle's
//! own NAVI CHANGE panel offers — has a tab of its own, laid out the
//! way the game's own PARTY STATUS card and its CUSTOM panel read
//! together: a panel per slot, who it fields on top, the party programs
//! they carry under that, and the gauge those fill below. The file pick
//! is only there while editing.
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
use tango_gamesupport_bn5ds_dataview::save::{self, Cross, Save, SaveSet};
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

/// Put a navi in a party slot, or empty it — the CHANGE the game's own
/// PARTY STATUS card offers. The dropdown only lists navis this file
/// recruited and not the other slot's, so no duplicate can be minted;
/// the save layer re-syncs the mirror bits the load checks a team
/// against, takes a departing member's programs back off, and packs the
/// pair the way the game's own machine compacts it (so emptying the
/// first slot moves the second up, loadout and all).
#[derive(Debug)]
struct SetPartyNavi {
    slot: usize,
    navi: Option<usize>,
}

impl GameEdit for SetPartyNavi {
    fn apply(&self, model: &mut tango_gamesupport_common::model::SaveModel) -> Invalidation {
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            save.set_team_navi(self.slot, self.navi);
            save.pack_team();
        }
        Invalidation::default()
    }
}

/// Put one more of a party program on a slot's member, exactly as the
/// PARTY CUSTOMIZER's own panel does: the member's record takes what
/// everything it equips adds up to, and the slot's entry takes the
/// programs. The offer is filtered by
/// [`Partycust::can_add`](save::Partycust::can_add), so nothing here can
/// outspend the member's gauge or the file's stock.
#[derive(Debug)]
struct AddPartyProgram {
    slot: usize,
    program: usize,
}

impl GameEdit for AddPartyProgram {
    fn apply(&self, model: &mut tango_gamesupport_common::model::SaveModel) -> Invalidation {
        let Some(assets) = model.assets.underlying_any().downcast_ref::<rom::Assets>() else {
            return Invalidation::default();
        };
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            let equipped = save::Partycust::new(save, assets, self.slot).with(self.program);
            save.set_party_programs(self.slot, equipped, assets);
        }
        Invalidation::default()
    }
}

/// Take the program a member equips in position `at` back off.
#[derive(Debug)]
struct RemovePartyProgram {
    slot: usize,
    at: usize,
}

impl GameEdit for RemovePartyProgram {
    fn apply(&self, model: &mut tango_gamesupport_common::model::SaveModel) -> Invalidation {
        let Some(assets) = model.assets.underlying_any().downcast_ref::<rom::Assets>() else {
            return Invalidation::default();
        };
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            let equipped = save::Partycust::new(save, assets, self.slot).without(self.at);
            save.set_party_programs(self.slot, equipped, assets);
        }
        Invalidation::default()
    }
}

/// A slot panel's clear-all: the member keeps its place and loses every
/// program, which is the customizer's own take-it-all-off.
#[derive(Debug)]
struct ClearPartyPrograms(usize);

impl GameEdit for ClearPartyPrograms {
    fn apply(&self, model: &mut tango_gamesupport_common::model::SaveModel) -> Invalidation {
        let Some(assets) = model.assets.underlying_any().downcast_ref::<rom::Assets>() else {
            return Invalidation::default();
        };
        if let Some(save) = model.save.as_any_mut().downcast_mut::<Save>() {
            save.set_party_programs(self.0, [], assets);
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

/// The same, for a dropdown that is an action rather than a state: it
/// never shows a selection, only what it would do.
fn action_pick_list<'a, T, M>(options: Vec<T>, prompt: String, on_select: impl Fn(T) -> M + 'a) -> iced::Element<'a, M>
where
    T: ToString + PartialEq + Clone + 'a,
    M: Clone + 'a,
{
    sweeten::widget::pick_list(options, Option::<T>::None, on_select)
        .placeholder(prompt)
        .padding(tango_gamesupport_common::style::CONTROL_PADDING)
        .text_size(tango_gamesupport_common::style::TEXT_BODY)
        .style(tango_gamesupport_common::widgets::chunky_pick_list)
        .into()
}

/// One navi a party slot could field, or the empty slot itself.
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

/// One party program the file could still put on a member, labeled the
/// way the customizer's own list labels it: the cart's name for it and
/// what it costs the gauge.
#[derive(Clone, PartialEq)]
struct ProgramChoice {
    index: usize,
    label: String,
}

impl std::fmt::Display for ProgramChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
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

/// The cart's name for navi `id` through the shared roster, or its
/// number for a cart that won't give one up.
fn navi_name(loaded: &OpenSave, navi: usize) -> String {
    loaded
        .assets
        .navi(navi)
        .and_then(|n| n.name())
        .unwrap_or_else(|| format!("#{navi}"))
}

/// A navi's emblem, at an integer multiple of its 15px crop so the
/// nearest-neighbour upscale lands on even pixels — the navi strip's own
/// sizing rule. `None` for a cart whose sheet would not read.
fn navi_emblem<'a>(loaded: &'a OpenSave, navi: usize, size: f32) -> Option<iced::Element<'a, Action>> {
    Some(
        iced::widget::Image::new(loaded.navi_emblems.get(&navi)?.clone())
            .filter_method(iced::widget::image::FilterMethod::Nearest)
            .width(iced::Length::Fixed(size))
            .height(iced::Length::Fixed(size))
            .into(),
    )
}

/// The navi whose emblem the identity card wears: MegaMan, the roster's
/// first, whoever the file dresses him as.
const MEGAMAN_NAVI: usize = 0;

/// The three numbers a navi's PARTY STATUS card shows: the HP it
/// brings, the ATTACK its card reads, and its chip attack.
///
/// All three are the card's own figures. The record keeps ATTACK
/// 0-based, so the card's is one more; the chip attack is what the navi
/// brings at this file's story rank plus whatever `P.Chp` programs it
/// carries.
fn navi_stats<'a>(lang: &'a LanguageIdentifier, loaded: &'a OpenSave, save: &'a Save, navi: usize) -> iced::Element<'a, Action> {
    row![
        sv::stat(
            tango_gamesupport_common::t!(lang, "navi-base-hp"),
            save.navi_hp(navi).to_string(),
        ),
        sv::stat(
            tango_gamesupport_common::t!(lang, "navi-buster-attack"),
            (save.partycust_bonus(navi).attack + 1).to_string(),
        ),
    ]
    .spacing(16)
    .align_y(iced::Alignment::End)
    .push_maybe(cart_of(loaded).map(|cart| {
        sv::stat(
            tango_gamesupport_common::t!(lang, "bn5ds-chip-attack"),
            save.navi_chip_attack(navi, cart).to_string(),
        )
    }))
    .into()
}

/// What the cart calls party program `index`.
fn program_name(loaded: &OpenSave, index: usize) -> String {
    cart_of(loaded)
        .and_then(|cart| cart.party_program(index))
        .and_then(|program| program.name())
        .unwrap_or_else(|| format!("#{index}"))
}

/// What the customizer's gauge paints a program's blocks: one colour
/// per family, read off the cart's own kind byte. The four are the
/// panel's, sampled from it — the cart keeps them in a UI palette
/// nothing indexes by kind, so the mapping lives here rather than
/// pretending to come out of the cartridge.
fn program_color(kind: Option<rom::PartyProgramKind>) -> iced::Color {
    match kind {
        Some(rom::PartyProgramKind::MaxHp) => iced::Color::from_rgb8(0xfb, 0x30, 0x20),
        Some(rom::PartyProgramKind::Attack) => iced::Color::from_rgb8(0xfb, 0xfb, 0x20),
        Some(rom::PartyProgramKind::ChipAttack) => iced::Color::from_rgb8(0x49, 0xa2, 0xfb),
        Some(rom::PartyProgramKind::Special) | None => iced::Color::from_rgb8(0xfb, 0xfb, 0xfb),
    }
}

/// How big an emblem is drawn: an integer multiple of its 15px crop, so
/// the nearest-neighbour upscale lands on even pixels. The identity
/// card wears one at the size BN5 and BN6's own navi cards do; a party
/// slot's is the smaller one its rows line up on.
const CARD_EMBLEM_SIZE: f32 = 45.0;
const EMBLEM_SIZE: f32 = 30.0;

/// How tall and wide one block of the gauge is drawn.
const GAUGE_BLOCK: f32 = 14.0;

/// How tall a program row is: the ✕ the edit session hangs on one, so
/// the rows do not change height when the session opens. Its icon at
/// iced's default 1.3 line height, plus the button's own padding, plus
/// the row's.
const PROGRAM_ROW_HEIGHT: f32 = tango_gamesupport_common::style::TEXT_BODY * 1.3 + 6.0 + 6.0;

/// How wide the accent stripe down a program row's left edge is — the
/// width the shared editor rows give their class accent.
const STRIPE_WIDTH: f32 = 6.0;

/// The member's gauge: one block per point of capacity, filled from the
/// left in the colour of whichever program paid for it, exactly as the
/// panel draws it.
fn partycust_gauge<'a>(loaded: &'a OpenSave, customizer: &save::Partycust) -> iced::Element<'a, Action> {
    let cart = cart_of(loaded);
    let mut filled: Vec<iced::Color> = Vec::new();
    for &index in customizer.equipped() {
        let Some(program) = cart.and_then(|cart| cart.party_program(index)) else { continue };
        let color = program_color(program.kind());
        filled.extend(std::iter::repeat_n(color, program.cost() as usize));
    }
    let mut blocks = row![].spacing(2).align_y(iced::Alignment::Center);
    for block in 0..customizer.capacity() as usize {
        let color = filled.get(block).copied();
        blocks = blocks.push(
            iced::widget::container(iced::widget::Space::new())
                .width(iced::Length::Fixed(GAUGE_BLOCK))
                .height(iced::Length::Fixed(GAUGE_BLOCK))
                .style(move |theme: &iced::Theme| iced::widget::container::Style {
                    background: Some(
                        color
                            .unwrap_or_else(|| {
                                let mut empty = theme.palette().text;
                                empty.a = 0.12;
                                empty
                            })
                            .into(),
                    ),
                    ..Default::default()
                }),
        );
    }
    blocks.into()
}

/// One party slot, as the game's own PARTY STATUS card and its CUSTOM
/// panel read together: the naming line, then the programs the member
/// carries over the gauge they fill.
///
/// Reading, the navi is a name and the programs are a plain list.
/// Editing, the navi is a dropdown over the file's recruited roster
/// (the CHANGE button's own offer), each program grows a ✕, and a
/// dropdown of what the file could still put on sits under them — the
/// gauge and the stock are already what that dropdown offers, so
/// nothing it lists can overspend either.
///
/// Returned as its two halves so each caller can hang them in the
/// container its side of the editor wants: see [`party_slot_pane`] and
/// [`party_slot_card`].
fn party_slot<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a OpenSave,
    save: &'a Save,
    slot: usize,
    editing: bool,
) -> (iced::Element<'a, Action>, iced::Element<'a, Action>) {
    let navi = save.team_navi(slot);
    let naming: iced::Element<'a, Action> = if editing {
        let held: Vec<usize> = (0..save::NUM_TEAM_SLOTS)
            .filter(|&other| other != slot)
            .filter_map(|other| save.team_navi(other))
            .collect();
        let mut options = vec![NaviChoice {
            navi: None,
            label: tango_gamesupport_common::t!(lang, "bn5ds-team-none"),
        }];
        options.extend(
            save.team_navi_choices()
                .into_iter()
                .filter(|choice| !held.contains(choice))
                .map(|choice| NaviChoice {
                    navi: Some(choice),
                    label: navi_name(loaded, choice),
                }),
        );
        let selected = options.iter().find(|choice| choice.navi == navi).cloned();
        pick_list(options, selected, move |choice: NaviChoice| {
            Action::Game(Arc::new(SetPartyNavi {
                slot,
                navi: choice.navi,
            }))
        })
    } else {
        match navi {
            Some(navi) => iced::widget::text(navi_name(loaded, navi))
                .size(tango_gamesupport_common::style::TEXT_BODY)
                .into(),
            None => iced::widget::text(tango_gamesupport_common::t!(lang, "bn5ds-team-none"))
                .size(tango_gamesupport_common::style::TEXT_BODY)
                .style(tango_gamesupport_common::widgets::muted_text_style)
                .into(),
        }
    };

    let Some(cart) = cart_of(loaded) else {
        return (
            iced::widget::container(naming)
                .width(iced::Fill)
                .padding(tango_gamesupport_common::style::HEADER_PADDING)
                .into(),
            iced::widget::Space::new().into(),
        );
    };
    let customizer = save::Partycust::new(save, cart, slot);

    // The gauge rides on the naming line, at the far end of it — not on
    // a row of its own, and not trailing the program list.
    let naming = row![]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .push(naming)
        .push(iced::widget::Space::new().width(iced::Fill))
        .push(partycust_gauge(loaded, &customizer))
        .push(
            iced::widget::text(format!("{} / {}", customizer.cost(), customizer.capacity()))
                .size(tango_gamesupport_common::style::TEXT_CAPTION)
                .style(tango_gamesupport_common::widgets::muted_text_style),
        )
        .push_maybe(editing.then(|| {
            sv::clear_all_button(lang, Action::Game(Arc::new(ClearPartyPrograms(slot))))
        }));
    let header = iced::widget::container(
        row![]
            .spacing(12)
            .align_y(iced::Alignment::Center)
            .push_maybe(navi.and_then(|navi| navi_emblem(loaded, navi, EMBLEM_SIZE)))
            .push(
                column![]
                    .spacing(4)
                    .width(iced::Fill)
                    .push(naming)
                    .push_maybe(navi.map(|navi| navi_stats(lang, loaded, save, navi))),
            ),
    )
    .width(iced::Fill)
    .padding(tango_gamesupport_common::style::HEADER_PADDING)
    .into();

    let mut body = column![].spacing(1).padding(0);
    for (at, &index) in customizer.equipped().iter().enumerate() {
        let color = program_color(cart.party_program(index).and_then(|program| program.kind()));
        let mut line = row![
            iced::widget::container(iced::widget::Space::new())
                .width(iced::Length::Fixed(STRIPE_WIDTH))
                .height(iced::Length::Fixed(PROGRAM_ROW_HEIGHT))
                .style(move |_: &iced::Theme| iced::widget::container::Style {
                    background: Some(color.into()),
                    ..Default::default()
                }),
            iced::widget::text(program_name(loaded, index))
                .size(tango_gamesupport_common::style::TEXT_BODY)
                .width(iced::Fill),
            sv::limit_caption(
                cart.party_program(index).map(|program| program.cost()).unwrap_or(0).to_string(),
                false,
            ),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        if editing {
            line = line.push(sv::remove_button(Action::Game(Arc::new(RemovePartyProgram { slot, at }))));
        }
        body = body.push(
            iced::widget::container(line)
                .width(iced::Fill)
                // Pinned so a row is the same height whether or not it
                // is carrying the edit session's ✕.
                .height(iced::Length::Fixed(PROGRAM_ROW_HEIGHT))
                .align_y(iced::Alignment::Center)
                .padding(iced::Padding::default().right(12.0))
                .style(tango_gamesupport_common::widgets::zebra_row(at)),
        );
    }
    if customizer.equipped().is_empty() {
        let empty = if navi.is_some() {
            tango_gamesupport_common::t!(lang, "bn5ds-partycust-empty")
        } else {
            tango_gamesupport_common::t!(lang, "bn5ds-team-none")
        };
        body = body.push(
            iced::widget::container(
                iced::widget::text(empty)
                    .size(tango_gamesupport_common::style::TEXT_BODY)
                    .style(tango_gamesupport_common::widgets::muted_text_style)
                    .width(iced::Fill),
            )
            .padding([3, 12]),
        );
    }
    if editing && navi.is_some() {
        let choices: Vec<ProgramChoice> = (0..tango_gamesupport_bn5ds_dataview::NUM_PARTY_PROGRAMS)
            .filter(|&index| customizer.can_add(index))
            .map(|index| ProgramChoice {
                index,
                label: format!(
                    "{} ({})",
                    program_name(loaded, index),
                    cart.party_program(index).map(|program| program.cost()).unwrap_or(0),
                ),
            })
            .collect();
        if !choices.is_empty() {
            let add = action_pick_list(
                choices,
                tango_gamesupport_common::t!(lang, "bn5ds-partycust-add"),
                move |choice: ProgramChoice| {
                    Action::Game(Arc::new(AddPartyProgram {
                        slot,
                        program: choice.index,
                    }))
                },
            );
            body = body.push(iced::widget::container(add).padding([6, 12]));
        }
    }

    (header, body.into())
}

/// One slot as the edit session hangs it: a full-height pane with its
/// own scrollbar, which is the shape the editors' two-pane row wants.
fn party_slot_pane<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a OpenSave,
    save: &'a Save,
    slot: usize,
) -> iced::Element<'a, Action> {
    let (header, body) = party_slot(lang, loaded, save, slot, true);
    sv::editor_pane(header, body)
}

/// One slot as the reading side hangs it: a plate that hugs what is on
/// it, which is what every read-only tab body is made of — they go
/// inside a shrink-height scrollable, where a full-height pane has
/// nothing to fill and collapses.
fn party_slot_card<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a OpenSave,
    save: &'a Save,
    slot: usize,
) -> iced::Element<'a, Action> {
    let (header, body) = party_slot(lang, loaded, save, slot, false);
    iced::widget::container(column![header, body].width(iced::Fill))
        .width(iced::Fill)
        .style(tango_gamesupport_common::widgets::pane)
        .into()
}

/// The Party tab's read-only body: the two slots the battle's NAVI
/// CHANGE panel offers, each over what the PARTY CUSTOMIZER gave it.
fn render_party<'a>(lang: &'a LanguageIdentifier, loaded: &'a OpenSave) -> iced::Element<'a, Action> {
    let Some(save) = file_of(loaded) else {
        return sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty"));
    };
    let mut slots = row![]
        .spacing(tango_gamesupport_common::style::PANE_GAP)
        .width(iced::Fill)
        .align_y(iced::Alignment::Start);
    for slot in 0..save::NUM_TEAM_SLOTS {
        slots = slots.push(party_slot_card(lang, loaded, save, slot));
    }
    slots.into()
}

/// The party customizer: one panel per slot, laid out the way the
/// game's own is — who the slot fields on top, the programs they carry
/// under it, the gauge below that.
fn render_party_edit<'a>(lang: &'a LanguageIdentifier, loaded: &'a OpenSave) -> iced::Element<'a, Action> {
    let Some(save) = file_of(loaded) else {
        return sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty"));
    };
    sv::editor_panes(
        party_slot_pane(lang, loaded, save, 0),
        party_slot_pane(lang, loaded, save, 1),
    )
}

/// The Party tab as clipboard text: one navi per line, with the card's
/// numbers and what the customizer has given it.
fn party_as_text(loaded: &OpenSave) -> Option<String> {
    let save = file_of(loaded)?;
    let cart = cart_of(loaded);
    Some(
        (0..save::NUM_TEAM_SLOTS)
            .filter_map(|slot| {
                let navi = save.team_navi(slot)?;
                let equipped = cart
                    .map(|cart| save::Partycust::new(save, cart, slot))
                    .map(|customizer| {
                        customizer
                            .equipped()
                            .iter()
                            .map(|&index| program_name(loaded, index))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                Some(format!(
                    "{}\t{}\t{}\t{equipped}",
                    navi_name(loaded, navi),
                    save.navi_hp(navi),
                    save.partycust_bonus(navi).attack + 1,
                ))
            })
            .collect::<Vec<_>>()
            .join("\n"),
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
/// over the file's team and the HP it brings. The party itself has its
/// own tab.
///
/// The caption line is on both halves, and the naming line is pinned to
/// [`CARD_HEIGHT`], so opening the edit session swaps one line for the
/// other without moving anything below it.
///
/// Carries the strip's own left inset, which the strip expects a card to
/// bring (see `render_identity_strip`).
fn card_slot<'a>(
    loaded: &'a OpenSave,
    inner: iced::Element<'a, Action>,
    team: iced::Element<'a, Action>,
    hp: Option<iced::Element<'a, Action>>,
) -> iced::Element<'a, Action> {
    let mut stats = row![team].spacing(16).align_y(iced::Alignment::End);
    if let Some(hp) = hp {
        stats = stats.push(hp);
    }
    let naming = iced::widget::container(inner)
        .height(iced::Length::Fixed(CARD_HEIGHT))
        .align_y(iced::Alignment::Center);
    // MegaMan's own emblem, beside whichever of his names the file
    // brings — the party's members wear theirs the same way.
    let card: iced::Element<'a, Action> = match navi_emblem(loaded, MEGAMAN_NAVI, CARD_EMBLEM_SIZE) {
        Some(emblem) => row![emblem, column![naming, stats].spacing(4)]
            .spacing(12)
            .align_y(iced::Alignment::Center)
            .into(),
        None => column![naming, stats].spacing(4).into(),
    };
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
        loaded,
        iced::widget::text(name)
            .size(tango_gamesupport_common::style::TEXT_TITLE)
            .wrapping(iced::widget::text::Wrapping::None)
            .into(),
        team_stat(lang, loaded, save),
        hp_stat(lang, loaded),
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
        loaded,
        pick_list(options, selected, |c: CrossChoice| {
            Action::Game(Arc::new(SetCross(c.cross)))
        }),
        team_stat(lang, loaded, save),
        hp_stat(lang, loaded),
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
        if file_of(loaded).is_some() {
            tabs.push(Tab::Party);
        }
        if save.view_auto_battle_data().is_some() {
            tabs.push(Tab::AutoBattleData);
        }
        tabs
    }

    /// The Party section is this game's own model rather than the
    /// shared one, so its editability is answered here: editable
    /// whenever the save is ours. Every other tab keeps the shared
    /// capability probe.
    fn tab_editable(&self, tab: Tab, loaded: &OpenSave) -> bool {
        match tab {
            Tab::Party => file_of(loaded).is_some(),
            tab => tab.editable_on(&loaded.editability),
        }
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
            Tab::Party => render_party(lang, loaded),
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
            Tab::Party => render_party_edit(lang, loaded),
            Tab::AutoBattleData => sv::abd::render_auto_battle_data_edit(lang, loaded, state),
            _ => sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty")),
        }
    }

    fn tab_as_text(&self, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
        match tab {
            Tab::Navicust => sv::navicust::navicust_as_text(loaded),
            Tab::Folder => sv::folder::as_text(loaded, opts),
            Tab::Party => party_as_text(loaded),
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
