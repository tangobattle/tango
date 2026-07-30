//! Headless smoke test for the Mod Card form: bakes an `OpenSave` from
//! the real ROM + a bundled save template, renders the read view and the
//! editor, and pushes slot edits through the `GameEdit` plumbing (which
//! exercises the save/assets downcasts end-to-end).
//!
//! Needs the ROM: set `TANGO_TEST_ROMS_DIR` to a directory holding
//! `bn4.gba` (BN4 Red Sun US, the golden-suite layout); skips silently
//! otherwise.

use std::sync::Arc;

use tango_gamesupport_bn4::ui::{PatchCard4Edit, SAVE_EDITOR};
use tango_gamesupport_common::editor::view::{Action, Outcome, RenderOpts, State, Tab};
use tango_gamesupport_common::editor::GameSaveEditor;

#[test]
fn mod_cards_render_and_edit_from_real_rom() {
    let Some(dir) = std::env::var_os("TANGO_TEST_ROMS_DIR") else {
        eprintln!("TANGO_TEST_ROMS_DIR not set; skipping");
        return;
    };
    let rom = std::fs::read(std::path::Path::new(&dir).join("bn4.gba")).expect("read bn4.gba");

    let game = &tango_gamesupport_bn4::BN4RS;
    let save = tango_gamesupport_common::dataview::unwrap_save(game.save_templates[0].1.clone_box());
    let model = tango_gamesupport_common::model::from_patched_rom(game, rom, std::path::PathBuf::new(), save, None);
    let mut loaded = tango_gamesupport_common::editor::loaded::from_model(model, &SAVE_EDITOR.0, &[game]);

    let lang = unic_langid::langid!("en-US");
    let tabs = SAVE_EDITOR.0.tabs(&loaded);
    assert!(tabs.contains(&Tab::PatchCards), "BN4 always has Mod Cards: {tabs:?}");
    // Mod Cards are editable even though the shared capability probe
    // can't see them (the model is BN4's own).
    assert!(SAVE_EDITOR.0.tab_editable(Tab::PatchCards, &loaded));
    assert!(!loaded.editability.patch_cards);
    // The downcasts the module relies on hold for a real loaded save.
    assert!(loaded
        .save
        .as_ref()
        .as_any()
        .downcast_ref::<tango_gamesupport_bn4::dataview::save::Save>()
        .is_some());
    let (card_id, card_slot) = {
        let assets = loaded
            .assets
            .underlying_any()
            .downcast_ref::<tango_gamesupport_bn4::dataview::rom::Assets>()
            .expect("downcast bn4 assets");
        (1..assets.num_patch_card4s())
            .find_map(|id| assets.patch_card4(id).map(|c| (id, c.slot)))
            .expect("a catalog card")
    };

    let mut state = State::new();
    state.enter_edit(&loaded);
    {
        let _read = SAVE_EDITOR
            .0
            .render(&lang, Tab::PatchCards, &loaded, RenderOpts::default());
        let _edit = SAVE_EDITOR.0.render_edit(&lang, Tab::PatchCards, &loaded, &state);
    }

    // Install a card the way the editor does: Action::Game surfaces as
    // Outcome::Edit, and apply_edit routes it back through the GameEdit
    // (downcasting to BN4's concrete save inside).
    let action = Action::Game(Arc::new(PatchCard4Edit::AddCard { id: card_id }));
    let (_task, outcome) = state.apply(&action, Some(&loaded));
    let Some(Outcome::Edit(edit)) = outcome else {
        panic!("Action::Game should surface as Outcome::Edit");
    };
    let _ = tango_gamesupport_common::model::apply_edit(&mut loaded.model, edit);

    let text = SAVE_EDITOR
        .0
        .tab_as_text(Tab::PatchCards, &loaded, RenderOpts::default())
        .expect("mod cards as text");
    let slot_label = ["0A", "0B", "0C", "0D", "0E", "0F"][card_slot as usize];
    assert!(
        text.contains(slot_label),
        "card {card_id} should occupy {slot_label}: {text:?}"
    );

    // And clear it again through the same seam.
    let (_task, outcome) = state.apply(
        &Action::Game(Arc::new(PatchCard4Edit::RemoveCard {
            slot: card_slot as usize,
        })),
        Some(&loaded),
    );
    let Some(Outcome::Edit(edit)) = outcome else {
        panic!("Action::Game should surface as Outcome::Edit");
    };
    let _ = tango_gamesupport_common::model::apply_edit(&mut loaded.model, edit);
    let text = SAVE_EDITOR
        .0
        .tab_as_text(Tab::PatchCards, &loaded, RenderOpts::default())
        .expect("mod cards as text");
    assert!(
        !text.contains(slot_label),
        "slot {slot_label} should be empty again: {text:?}"
    );
}
