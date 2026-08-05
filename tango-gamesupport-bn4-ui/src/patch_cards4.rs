//! BN4's Mod Card (patch card 4) form: six fixed catalog slots (0A–0F),
//! each holding at most one card. The whole model is BN4's own —
//! [`crate::dataview`] exposes it concretely, and this
//! module reaches it by downcasting the shared save/assets handles
//! (`AsAny` / `underlying_any`). Edits ship as a [`GameEdit`] through
//! the shared action plumbing.

use std::sync::Arc;

use iced::widget::{container, scrollable, text, Space};
use iced::{Alignment, Element, Fill, Length};
use sweeten::widget::{column, pick_list, row};
use tango_gamesupport_bn4_dataview::rom::{self as bn4_rom, PatchCard4Effect};
use tango_gamesupport_bn4_dataview::save as bn4_save;
use tango_gamesupport_common_ui::editor::loaded::OpenSave;
use tango_gamesupport_common_ui::editor::view::{
    edit_toggle_maybe, editor_header, patch_cards, placeholder, Action, State,
};
use tango_gamesupport_common_ui::model::edit::{GameEdit, Invalidation};
use tango_gamesupport_common_ui::style::{self, TEXT_BODY, TEXT_CAPTION};
use tango_gamesupport_common_ui::t;
use tango_gamesupport_common_ui::widgets::{self, muted_text_style};
use unic_langid::LanguageIdentifier;

/// BN4 catalog-slot labels (the "0A"–"0F" the game shows). A BN4 patch
/// card belongs to exactly one of these six slots, and a slot holds at most
/// one card — so the editor is a per-slot picker, not the BN5/BN6 list.
const PATCH_CARD4_SLOT_LABELS: [&str; 6] = ["0A", "0B", "0C", "0D", "0E", "0F"];

/// The loaded save as BN4's concrete save. `None` only if the registry
/// wired this UI to a non-BN4 game, which would be a bug — callers
/// render the empty placeholder then rather than panicking.
fn bn4_save(loaded: &OpenSave) -> Option<&bn4_save::Save> {
    loaded.save.as_ref().as_any().downcast_ref::<bn4_save::Save>()
}

/// The loaded assets as BN4's concrete assets, through any override
/// layering (Mod Cards aren't patch-overridable, so nothing is lost).
fn bn4_assets(loaded: &OpenSave) -> Option<&bn4_rom::Assets> {
    loaded.assets.underlying_any().downcast_ref::<bn4_rom::Assets>()
}

/// A card's display name, falling back to its catalog number.
fn card_name(info: Option<&bn4_rom::PatchCard4Info>, id: usize) -> String {
    match info.map(|i| i.name).filter(|n| !n.is_empty()) {
        Some(name) => name.to_string(),
        None => format!("#{id}"),
    }
}

/// The read-only Mod Card list: a slot badge + the card's "name — effect"
/// line, with the bug (if any) in purple beneath.
pub fn render<M: 'static>(lang: &LanguageIdentifier, loaded: &OpenSave) -> Element<'static, M> {
    let (Some(save), Some(assets)) = (bn4_save(loaded), bn4_assets(loaded)) else {
        return placeholder(t!(lang, "save-empty"));
    };
    let v = save.view_patch_card4s();

    let mut list = column![].spacing(3).padding(0);
    for (slot, slot_label) in PATCH_CARD4_SLOT_LABELS.iter().enumerate() {
        let badge: Element<'static, M> = container(text(*slot_label).size(TEXT_BODY).font(iced::Font::MONOSPACE))
            .width(Length::Fixed(34.0))
            .align_x(iced::alignment::Horizontal::Center)
            .into();
        let cell: Element<'static, M> = match v.patch_card(slot) {
            Some(card) => {
                let info = assets.patch_card4(card.id);
                let name = card_name(info.as_ref(), card.id);
                // 3-digit catalog number, then the "name — effect"
                // line (name struck + everything muted when off).
                let number = text(format!("{:03}", card.id))
                    .size(TEXT_BODY)
                    .font(iced::Font::MONOSPACE)
                    .style(muted_text_style);
                let label = patch_cards::patch_card_name(
                    match info.as_ref().map(|i| i.effect) {
                        Some(effect) => format!("{name} — {}", effect_label(effect)),
                        None => name,
                    },
                    card.enabled,
                );
                let mut col = column![row![badge, number, container(label).width(Length::Fill)]
                    .spacing(8)
                    .align_y(Alignment::Center)]
                .spacing(2);
                if let Some(bug) = info.as_ref().and_then(|i| bugs_label(i.bugs)) {
                    col = col.push(
                        row![
                            Space::new().width(Length::Fixed(44.0)),
                            text(bug)
                                .size(TEXT_BODY)
                                .color(iced::Color::from_rgb8(0xb5, 0x5a, 0xde)),
                        ]
                        .spacing(0),
                    );
                }
                col.into()
            }
            None => row![
                badge,
                text(t!(lang, "patch-card4-none"))
                    .size(TEXT_BODY)
                    .style(muted_text_style)
                    .width(Length::Fill),
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
        };
        list = list.push(
            container(cell)
                .width(Fill)
                .padding([8, 10])
                .style(widgets::zebra_row(slot)),
        );
    }

    container(list).width(Fill).style(widgets::pane).into()
}

/// One slot's row in the editor: the slot label, a dropdown of every
/// card that belongs to this slot (plus "None" to empty it), an ON/off
/// toggle for the installed card, and — since a card's downside isn't in
/// the dropdown label — its bug line in purple underneath.
fn slot_row<'a>(
    assets: &bn4_rom::Assets,
    slot: usize,
    installed: Option<tango_gamesupport_common_dataview::save::PatchCard>,
    choices: Vec<PatchCard4Choice>,
) -> Element<'a, Action> {
    let badge = container(
        text(PATCH_CARD4_SLOT_LABELS[slot])
            .size(TEXT_BODY)
            .font(iced::Font::MONOSPACE),
    )
    .width(Length::Fixed(34.0))
    .align_x(iced::alignment::Horizontal::Center);

    let selected_id = installed.as_ref().map(|c| c.id);
    let selected = choices.iter().find(|c| c.id == selected_id).cloned();
    let picker = pick_list(choices, selected, move |c: PatchCard4Choice| match c.id {
        Some(id) => Action::Game(Arc::new(PatchCard4Edit::AddCard { id })),
        None => Action::Game(Arc::new(PatchCard4Edit::RemoveCard { slot })),
    })
    .width(Fill)
    .padding(style::CONTROL_PADDING)
    .text_size(TEXT_BODY)
    .style(widgets::chunky_pick_list);

    // The ON toggle shows on every row (so the column stays aligned); an
    // empty slot has nothing to enable, so it renders disabled (greyed,
    // unclickable). Green matches the other editors' "on" tint.
    let toggle = edit_toggle_maybe(
        "ON",
        installed.as_ref().is_some_and(|c| c.enabled),
        iced::Color::from_rgb8(0x29, 0xa1, 0x21),
        installed
            .as_ref()
            .map(|_| Action::Game(Arc::new(PatchCard4Edit::ToggleCard { slot }))),
    );
    let top = row![badge, picker, toggle].spacing(10).align_y(Alignment::Center);

    let mut cell = column![top].spacing(2);
    // Bug line for the installed card, aligned under the dropdown (past the
    // slot badge). The effect is already in the dropdown label; the bug is
    // the downside the user should still see at a glance.
    if let Some(bug) = installed
        .as_ref()
        .and_then(|c| assets.patch_card4(c.id))
        .and_then(|i| bugs_label(i.bugs))
    {
        cell = cell.push(
            row![
                Space::new().width(Length::Fixed(44.0)),
                text(bug)
                    .size(TEXT_BODY)
                    .color(iced::Color::from_rgb8(0xb5, 0x5a, 0xde)),
            ]
            .spacing(0),
        );
    }
    container(cell)
        .width(Fill)
        .padding([8, 10])
        .style(widgets::zebra_row(slot))
        .into()
}

/// The Mod Card editor: the six catalog slots (0A–0F) as a single form.
/// Each slot has a dropdown of the cards that belong to it (plus
/// "None"), so the model is "pick one card per slot" — matching the in-game
/// Mod Card screen — rather than the BN5/BN6 collection-from-a-library.
/// There's no MB budget. Edits stage live in the loaded save and are
/// written to disk only on Save.
pub fn render_edit<'a>(lang: &'a LanguageIdentifier, loaded: &'a OpenSave, state: &'a State) -> Element<'a, Action> {
    // Only reached while editing, so the EditState is present.
    if state.editing.is_none() {
        return placeholder(t!(lang, "save-empty"));
    }
    let (Some(save), Some(assets)) = (bn4_save(loaded), bn4_assets(loaded)) else {
        return placeholder(t!(lang, "save-empty"));
    };
    let v = save.view_patch_card4s();

    // Bucket every card id by the slot it belongs to (one pass), so each
    // slot's dropdown lists only its own cards.
    let mut by_slot: [Vec<usize>; PATCH_CARD4_SLOT_LABELS.len()] = std::array::from_fn(|_| Vec::new());
    for id in 0..assets.num_patch_card4s() {
        if let Some(info) = assets.patch_card4(id) {
            let s = info.slot as usize;
            if let Some(bucket) = by_slot.get_mut(s) {
                bucket.push(id);
            }
        }
    }

    let mut rows = column![].spacing(3).padding(0);
    let mut filled = 0usize;
    for (slot, ids) in by_slot.iter().enumerate() {
        let installed = v.patch_card(slot);
        if installed.is_some() {
            filled += 1;
        }
        let mut choices = vec![PatchCard4Choice::none(lang)];
        choices.extend(ids.iter().map(|&id| PatchCard4Choice::card(assets, id)));
        rows = rows.push(slot_row(assets, slot, installed, choices));
    }

    let count_caption = text(t!(lang, "patch-card-edit-count", count = filled as i64))
        .size(TEXT_CAPTION)
        .style(muted_text_style);
    let header = editor_header(
        lang,
        t!(lang, "save-tab-patch-cards"),
        vec![count_caption.into()],
        Action::Game(Arc::new(PatchCard4Edit::ClearAll)),
    );

    container(column![
        header,
        scrollable(rows)
            .style(widgets::chunky_scrollable)
            .height(Fill)
            .width(Fill)
    ])
    .width(Fill)
    .height(Fill)
    .style(widgets::pane)
    .into()
}

/// The Mod Card tab as TSV text.
pub fn as_text(loaded: &OpenSave) -> Option<String> {
    let save = bn4_save(loaded)?;
    let assets = bn4_assets(loaded)?;
    let v = save.view_patch_card4s();
    let mut out = String::new();
    for i in 0..6 {
        let Some(card) = v.patch_card(i) else { continue };
        if !card.enabled {
            continue;
        }
        let name = card_name(assets.patch_card4(card.id).as_ref(), card.id);
        out.push_str(&format!("0{}\t{name}\n", ['A', 'B', 'C', 'D', 'E', 'F'][i],));
    }
    Some(out)
}

/// A staged Mod Card edit. BN4 is slot-based: every card belongs to one
/// fixed catalog slot, so adding routes the card to its own slot
/// (replacing whatever was there). No MB budget, no list shifting.
/// Ships through the shared plumbing as a [`GameEdit`]; `apply`
/// downcasts back to BN4's concrete save and rebuilds the anti-cheat
/// mirror, so commit only has to checksum + write.
#[derive(Debug, Clone)]
pub enum PatchCard4Edit {
    /// Install card `id` into its own catalog slot, enabled.
    AddCard { id: usize },
    /// Empty catalog slot `slot`.
    RemoveCard { slot: usize },
    /// Toggle slot `slot`'s card between enabled and disabled.
    ToggleCard { slot: usize },
    /// Empty every slot.
    ClearAll,
}

impl GameEdit for PatchCard4Edit {
    fn apply(&self, save: &mut tango_gamesupport_common_ui::model::SaveModel) -> Invalidation {
        use tango_gamesupport_common_dataview::save::PatchCard;

        // The card's home slot resolves through the ROM catalog; read it
        // before the save is borrowed mutably.
        let add_slot = match self {
            PatchCard4Edit::AddCard { id } => {
                let Some(slot) = save
                    .assets
                    .underlying_any()
                    .downcast_ref::<bn4_rom::Assets>()
                    .and_then(|a| a.patch_card4(*id))
                    .map(|c| c.slot as usize)
                    .filter(|&s| s < PATCH_CARD4_SLOT_LABELS.len())
                else {
                    return Invalidation::default();
                };
                Some(slot)
            }
            _ => None,
        };

        let Some(bn4) = save.save.as_mut().as_any_mut().downcast_mut::<bn4_save::Save>() else {
            return Invalidation::default();
        };
        let mut v = bn4.view_patch_card4s_mut();

        match self {
            PatchCard4Edit::AddCard { id } => {
                v.set_patch_card(add_slot.unwrap(), Some(PatchCard { id: *id, enabled: true }));
            }
            PatchCard4Edit::RemoveCard { slot } => {
                v.set_patch_card(*slot, None);
            }
            PatchCard4Edit::ToggleCard { slot } => {
                let Some(c) = v.patch_card(*slot) else {
                    return Invalidation::default();
                };
                v.set_patch_card(
                    *slot,
                    Some(PatchCard {
                        id: c.id,
                        enabled: !c.enabled,
                    }),
                );
            }
            PatchCard4Edit::ClearAll => {
                for slot in 0..PATCH_CARD4_SLOT_LABELS.len() {
                    v.set_patch_card(slot, None);
                }
            }
        }

        // Keep the anti-cheat mirror in sync with the edit.
        v.rebuild_anticheat();
        Invalidation::default()
    }
}

/// A choice in a slot's card dropdown: the card id (`None` clears the
/// slot) plus a pre-resolved label. The label folds the card's effect into
/// the name (`"Max HP Up — Max HP+100"`), since within one slot several
/// cards share a name and only the effect tells them apart. `Display`
/// renders the label; equality is by id so the picker can match the
/// currently-installed card.
#[derive(Clone)]
struct PatchCard4Choice {
    id: Option<usize>,
    label: String,
}

impl PatchCard4Choice {
    fn none(lang: &LanguageIdentifier) -> Self {
        Self {
            id: None,
            label: t!(lang, "patch-card4-none"),
        }
    }

    fn card(assets: &bn4_rom::Assets, id: usize) -> Self {
        let info = assets.patch_card4(id);
        let name = card_name(info.as_ref(), id);
        // 3-digit catalog number prefix (also disambiguates same-named
        // cards in the dropdown); then the effect to tell them apart.
        let label = format!(
            "{id:03} {name} — {}",
            effect_label(info.as_ref().map_or(PatchCard4Effect::None, |c| c.effect))
        );
        Self { id: Some(id), label }
    }
}

impl PartialEq for PatchCard4Choice {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::fmt::Display for PatchCard4Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Human-readable label for a BN4 patch-card effect, derived from the
/// machine-readable [`PatchCard4Effect`] decoded out of the ROM.
/// (B-shortcut chip params are shown raw for now — the shortcut →
/// chip-id table isn't mapped yet.)
fn effect_label(effect: PatchCard4Effect) -> String {
    use bn4_rom::{
        PatchCard4Aura as A, PatchCard4Color as C, PatchCard4Effect as E, PatchCard4Panel as P,
        PatchCard4PetColor as PT, PatchCard4Soul as S,
    };
    match effect {
        E::None => "—".to_string(),
        E::PetMenu(c) => format!(
            "{} PET menu",
            match c {
                PT::Blue => "Blue",
                PT::Pink => "Pink",
                PT::Green => "Green",
                PT::Black => "Black",
            }
        ),
        E::MaxHp(n) => format!("Max HP +{n}"),
        E::BusterAttack(n) => format!("Buster Attack {}", n as u16 + 1),
        E::BButton(s) => format!("B Button {s:?}"),
        E::BCharge(s) => format!("B Charge {s:?}"),
        E::BLeft(s) => format!("B + ← {s:?}"),
        E::CustomSlots(n) => format!("Custom +{n}"),
        E::MegaFolder(n) => format!("Mega Chip +{n}"),
        E::GigaFolder(n) => format!("Giga Chip +{n}"),
        E::TripleSupporter => "Triple Supporter".to_string(),
        E::PanelStep(p) => format!(
            "{} Panel Step",
            match p {
                P::Broken => "Broken",
                P::Cracked => "Cracked",
                P::Metal => "Metal",
                P::Holy => "Holy",
            }
        ),
        E::FullSynchro => "Full Synchro".to_string(),
        E::Aura(a) => match a {
            A::Barrier100 => "Barrier 100",
            A::Barrier200 => "Barrier 200",
            A::LifeAura => "LifeAura",
        }
        .to_string(),
        E::Soul(s) => format!(
            "{} Soul",
            match s {
                S::Roll => "Roll",
                S::Guts => "Guts",
                S::Wind => "Wind",
                S::Search => "Search",
                S::Fire => "Fire",
                S::Thunder => "Thunder",
                S::Proto => "Proto",
                S::Number => "Number",
                S::Metal => "Metal",
                S::Junk => "Junk",
                S::Aqua => "Aqua",
                S::Wood => "Wood",
            }
        ),
        E::Color(c) => format!(
            "{} MegaMan",
            match c {
                C::Red => "Red",
                C::Yellow => "Yellow",
                C::White => "White",
                C::Green => "Green",
            }
        ),
        E::AllGuard => "All Guard".to_string(),
    }
}

/// Joined human-readable label for a card's bugs, or `None` if it has none.
fn bugs_label(bugs: &[bn4_rom::PatchCard4Bug]) -> Option<String> {
    use bn4_rom::PatchCard4Bug as B;
    if bugs.is_empty() {
        return None;
    }
    Some(
        bugs.iter()
            .map(|b| match b {
                B::Confused => "Start battle Confused",
                B::AutoMove => "Auto-move forward",
                B::Hp(_) => "HP Bug",
                B::CustomHP => "Custom HP Bug",
                B::CustomMinus1 => "Custom −1",
                B::PoisonPanelStep => "Poison Panel Step",
            })
            .collect::<Vec<_>>()
            .join(" & "),
    )
}
