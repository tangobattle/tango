//! Headless smoke test for the program deck view: bakes an `OpenSave`
//! from the real ROM + the crate's bundled save template, renders the
//! read view and the editor (iced element construction is pure — no
//! window needed), and pushes a slot edit through the savemodel path.
//!
//! Needs the ROM: set `TANGO_TEST_ROMS_DIR` to a directory holding
//! `bcc.gba` (the golden-suite layout); skips silently otherwise.

use tango_gamesupport_bcc::ui::SAVE_EDITOR;
use tango_gamesupport_common::editor::view::{RenderOpts, State, Tab};
use tango_gamesupport_common::editor::GameSaveEditor;

#[test]
fn renders_program_deck_from_real_rom() {
    let Some(dir) = std::env::var_os("TANGO_TEST_ROMS_DIR") else {
        eprintln!("TANGO_TEST_ROMS_DIR not set; skipping");
        return;
    };
    let rom = std::fs::read(std::path::Path::new(&dir).join("bcc.gba")).expect("read bcc.gba");

    let game = &tango_gamesupport_bcc::BCC;
    // The bundled template: a blank slate — MegaMan, an empty board and
    // an empty folder, for building a deck from scratch in the editor.
    let save = tango_gamesupport_common::dataview::unwrap_save(game.save_templates[0].1.clone_box());
    let model =
        tango_gamesupport_common::model::from_patched_rom(game, rom.clone(), std::path::PathBuf::new(), save, None);
    let mut loaded = tango_gamesupport_common::editor::loaded::from_model(model, &SAVE_EDITOR.0, &[game]);

    let lang = unic_langid::langid!("en-US");
    assert_eq!(SAVE_EDITOR.0.tabs(&loaded), vec![Tab::ProgramDeck]);

    // Read view + clipboard text: the template board is empty, so the
    // only line is the navi and the wired total is zero. The elements
    // borrow `loaded`, so they're scoped to drop before the edits below
    // take it mutably.
    {
        let _read = SAVE_EDITOR
            .0
            .render(&lang, Tab::ProgramDeck, &loaded, RenderOpts::default());
    }
    let text = SAVE_EDITOR
        .0
        .tab_as_text(Tab::ProgramDeck, &loaded, RenderOpts::default())
        .expect("deck as text");
    assert!(text.contains("MegaMan"), "template deck's navi is MegaMan: {text:?}");
    assert!(
        text.contains("0/330MB"),
        "template deck: nothing wired against MegaMan's 170 + the finished save's 160 bonus: {text:?}"
    );
    assert_eq!(
        text.lines().count(),
        3,
        "a blank template is the navi line plus the two budget lines — no slots: {text:?}"
    );
    assert!(
        text.contains("Slot-in\t\t80MB"),
        "each trigger slot may hold up to the finished save's 80MB: {text:?}"
    );

    // The concrete chip record: the game's own card text (probe-
    // verified table — see dataview::rom) and the stats the library
    // shows as columns.
    {
        let bcc = loaded
            .assets
            .underlying_any()
            .downcast_ref::<tango_gamesupport_bcc::dataview::rom::Assets>()
            .expect("concrete BCC assets");
        let cannon = bcc.chip_info(1).expect("Cannon exists");
        assert_eq!(
            cannon.description().as_deref(),
            Some("None\nAccC Norm60\nNavichip Attack"),
            "Cannon card text, one string per line as the P.DATA box draws it"
        );
        assert_eq!((cannon.hp(), cannon.attack_power(), cannon.mb()), (60, 60, 10));
        // MB is a u16 in the game: HubStyl's 350 doesn't fit a byte, and
        // truncating it would misprice a deck built on it.
        assert_eq!(bcc.chip_info(234).map(|c| c.mb()), Some(350));
        // Elements come off the chip data's kind word, and each has the
        // indicator icon the chip list draws (none/fire/aqua/wood/elec).
        assert_eq!(cannon.element(), 0, "Cannon is elementless");
        assert_eq!(bcc.chip_info(91).map(|c| c.element()), Some(1), "Meteor3 is fire");
        for element in 0..tango_gamesupport_bcc::dataview::rom::NUM_ELEMENTS {
            let icon = tango_gamesupport_common::dataview::rom::Assets::element_icon(bcc, element)
                .unwrap_or_else(|| panic!("element {element} has an icon"));
            assert_eq!(icon.dimensions(), (16, 16));
        }
        assert!(
            tango_gamesupport_common::dataview::rom::Assets::element_icon(
                bcc,
                tango_gamesupport_bcc::dataview::rom::NUM_ELEMENTS
            )
            .is_none(),
            "no icon past the last element"
        );
        // The description font's own glyphs (screen-pinned): Meteor3's
        // accuracy '?' and Recov50's "HP!".
        let meteor = bcc.chip_info(91).expect("Meteor3 exists");
        assert_eq!(
            meteor.description().as_deref(),
            Some("Fire\nAcc? Meteo40\nNvChip 3 Rnd Atk")
        );
        let recov = bcc.chip_info(119).expect("Recov50 exists");
        assert_eq!(
            recov.description().as_deref(),
            Some("None Acc_\nRecov50\nRegain 50 HP!")
        );
    }

    // The JP description font: kanji block + shared symbols, pinned
    // against the JP game's own P.DATA boxes.
    if let Ok(jp_rom) = std::fs::read(std::path::Path::new(&dir).join("bcgp.gba")) {
        let jp = tango_gamesupport_bcc::dataview::rom::Assets::new(
            &tango_gamesupport_bcc::dataview::rom::A89J_00,
            tango_gamesupport_bcc::dataview::rom::JA_CHARSET,
            jp_rom,
            vec![0; 0x40000],
        );
        let cannon = jp.chip_info(1).expect("Cannon exists");
        assert_eq!(
            cannon.description().as_deref(),
            Some("無属性 命中率C ノーマル60\nナビチップに攻撃"),
            "JP Cannon card text"
        );
        assert_eq!(jp.chip_info(242).and_then(|c| c.name()).as_deref(), Some("ノーマルナビ4"));
        let megaman = jp.chip_info(200).expect("MegaMan exists");
        assert_eq!(
            megaman.description().as_deref(),
            Some("無属性 命中率A かいひ率B\nチャージショット 無属性\n後列バトルチップに ついか攻撃"),
            "JP MegaMan card text, as the box draws it"
        );
        assert_no_undecoded(&jp, "jp");
    } else {
        eprintln!("bcgp.gba not present; skipping the JP description check");
    }

    // Nothing the UI shows may fall off the end of a charset: the
    // tables are pinned to what the games actually draw (see
    // dataview::rom), so a replacement char here means a real gap.
    {
        let bcc = loaded
            .assets
            .underlying_any()
            .downcast_ref::<tango_gamesupport_bcc::dataview::rom::Assets>()
            .expect("concrete BCC assets");
        assert_no_undecoded(bcc, "us");
    }

    // Editor view.
    let mut state = State::new();
    state.enter_edit(&loaded);
    {
        let _edit = SAVE_EDITOR.0.render_edit(&lang, Tab::ProgramDeck, &loaded, &state);
    }

    // Slot-targeted edits through the model, onto the blank board: drop
    // a chip into the L slot (ChipsView slot 10 = deck position 11) and
    // Cannon into position 1, then clear position 1 again.
    use tango_gamesupport_common::model::edit::{apply_edit, ChipEdit, Edit};
    let star = |id| {
        Some(tango_gamesupport_common::dataview::save::Chip {
            id,
            code: tango_gamesupport_common::dataview::save::ChipCode::Star,
        })
    };
    let _ = apply_edit(
        &mut loaded.model,
        Edit::Chips(ChipEdit::SetChip { slot: 10, chip: star(16) }),
    );
    let _ = apply_edit(
        &mut loaded.model,
        Edit::Chips(ChipEdit::SetChip { slot: 0, chip: star(1) }),
    );
    let text = SAVE_EDITOR
        .0
        .tab_as_text(Tab::ProgramDeck, &loaded, RenderOpts::default())
        .expect("deck as text");
    assert!(text.contains("L\t"), "L slot filled after SetChip: {text:?}");
    assert!(text.contains("1\tCannon"), "position 1 filled after SetChip: {text:?}");
    let _ = apply_edit(
        &mut loaded.model,
        Edit::Chips(ChipEdit::SetChip { slot: 0, chip: None }),
    );
    let text = SAVE_EDITOR
        .0
        .tab_as_text(Tab::ProgramDeck, &loaded, RenderOpts::default())
        .expect("deck as text");
    assert!(text.contains("L\t"), "clearing position 1 left the L slot alone: {text:?}");
    assert!(!text.contains("1\tCannon"), "position 1 cleared: {text:?}");

    // Navi replacement: the navi socket is slot NAVI_SLOT. A program
    // chip is refused there; a navi chip swaps the deck's navi (and its
    // MB capacity with it — Roll's 140 against MegaMan's 170).
    use tango_gamesupport_bcc::dataview::save::NAVI_SLOT;
    let _ = apply_edit(
        &mut loaded.model,
        Edit::Chips(ChipEdit::SetChip {
            slot: NAVI_SLOT,
            chip: Some(tango_gamesupport_common::dataview::save::Chip {
                id: 16,
                code: tango_gamesupport_common::dataview::save::ChipCode::Star,
            }),
        }),
    );
    let text = SAVE_EDITOR
        .0
        .tab_as_text(Tab::ProgramDeck, &loaded, RenderOpts::default())
        .expect("deck as text");
    assert!(text.contains("MegaMan"), "program chip refused as navi: {text:?}");
    let _ = apply_edit(
        &mut loaded.model,
        Edit::Chips(ChipEdit::SetChip {
            slot: NAVI_SLOT,
            chip: Some(tango_gamesupport_common::dataview::save::Chip {
                id: 201,
                code: tango_gamesupport_common::dataview::save::ChipCode::Star,
            }),
        }),
    );
    let text = SAVE_EDITOR
        .0
        .tab_as_text(Tab::ProgramDeck, &loaded, RenderOpts::default())
        .expect("deck as text");
    assert!(text.contains("Navi\tRoll"), "navi swapped to Roll: {text:?}");
    assert!(
        text.contains("/300MB"),
        "capacity follows the navi (Roll's 140 + the 160 bonus): {text:?}"
    );

    // The same swap through the editor's own action pipeline (what a
    // click in the library actually sends): select the navi card, pick
    // GutsMan, apply the produced edit.
    use tango_gamesupport_common::editor::view::{Action, Outcome};
    let (_, out) = state.apply(&Action::SelectDeckSlot(Some(NAVI_SLOT)), Some(&loaded));
    assert!(out.is_none());
    assert_eq!(
        state.editing.as_ref().and_then(|e| e.selected_deck_slot),
        Some(NAVI_SLOT),
        "navi card selection sticks"
    );
    let (_, out) = state.apply(
        &Action::SetDeckChip {
            slot: NAVI_SLOT,
            chip_id: 202,
            code: tango_gamesupport_common::dataview::save::ChipCode::Star,
        },
        Some(&loaded),
    );
    let Some(Outcome::Edit(edit)) = out else {
        panic!("SetDeckChip at the navi socket produces an edit");
    };
    let _ = apply_edit(&mut loaded.model, edit);
    let text = SAVE_EDITOR
        .0
        .tab_as_text(Tab::ProgramDeck, &loaded, RenderOpts::default())
        .expect("deck as text");
    assert!(text.contains("Navi\tGutsMan"), "navi swapped via actions: {text:?}");

    // The save strip beside Play reads the navi's HP off the navi view,
    // which follows the equipped navi chip (GutsMan's own HP stat).
    {
        {
            let nv = loaded.model.save.view_navi().expect("BCC reports a navi");
            assert_eq!(nv.navi(), 202, "navi view follows the socket");
            assert_eq!(nv.max_hp(&*loaded.model.assets), 600, "GutsMan's HP");
        }
        assert!(
            loaded.model.save.view_navi_mut().is_none(),
            "read-only: the deck board owns navi swapping, not the shared picker"
        );
    }

    // The swap must land the way the game stores a navi (probe-verified
    // recipe): the GutsMan chip minted into a folder entry, bound at
    // deck-array position 0, with the fallback byte synced.
    let wram = loaded.model.save.as_raw_wram();
    let deck = &wram[0x36c..0x36c + 0x90];
    assert_eq!(deck[1], 202, "fallback byte synced to GutsMan");
    let entry = deck[2] as usize;
    assert!(entry < 30, "position 0 bound to a folder entry: {:#x}", deck[2]);
    assert_eq!(deck[0x14 + entry * 4], 202, "bound entry holds the GutsMan chip");
    assert_eq!(deck[0x14 + entry * 4 + 1], 0, "entry back-reference is position 0");

    // A full folder must not make the library inert. The deck is an
    // equip view over the Navi's thirty-entry folder, so equipping a
    // chip the folder has no copy of has to restock an entry — and one
    // is always free, since the eleven board slots plus the navi socket
    // can only ever claim twelve of the thirty. Before this, `set_chip`
    // returned false and the edit vanished: the library row looked
    // addable and clicking it did nothing.
    {
        // Pack the folder out with Cannon; the grow loop stops when it
        // runs out of empty entries, which is the state under test.
        {
            let mut v = loaded.model.save.view_chips_mut().expect("chips view");
            let _ = v.set_pack_count(1, 0, 30);
        }
        let held: usize = (1..=255)
            .filter_map(|id| loaded.model.save.view_chips().unwrap().pack_count(id, 0))
            .sum();
        assert_eq!(held, 30, "folder packed full for the test");

        // Recov10 (0MB, and absent from the packed folder) into position 1.
        let _ = apply_edit(
            &mut loaded.model,
            Edit::Chips(ChipEdit::SetChip {
                slot: 0,
                chip: Some(tango_gamesupport_common::dataview::save::Chip {
                    id: 117,
                    code: tango_gamesupport_common::dataview::save::ChipCode::Star,
                }),
            }),
        );
        let v = loaded.model.save.view_chips().expect("chips view");
        assert_eq!(
            v.chip(0, 0).map(|c| c.id),
            Some(117),
            "equipping into a full folder restocks an unused entry instead of no-opping"
        );
        assert_eq!(
            v.chip(0, NAVI_SLOT).map(|c| c.id),
            Some(202),
            "restocking took an unequipped entry, not the navi's"
        );
        let held: usize = (1..=255).filter_map(|id| v.pack_count(id, 0)).sum();
        assert_eq!(held, 30, "the folder still holds thirty chips, one of them now Recov10");
    }

    // The editor follows the *equipped* deck, not deck 0. A save carries
    // three, and which one the game fields is the byte just past the
    // third block (probe-verified — see dataview::save). Point it at
    // deck 1, wire Cannon into that deck's position 1, and the board has
    // to show deck 1's contents rather than the deck 0 built above.
    {
        use tango_gamesupport_bcc::dataview::save::EQUIPPED_DECK_OFFSET;
        use tango_gamesupport_common::dataview::save::Save as _;
        let mut wram = loaded.model.save.as_raw_wram().into_owned();
        wram[EQUIPPED_DECK_OFFSET] = 1;
        let switched = tango_gamesupport_bcc::dataview::save::Save::from_wram(&wram).expect("save re-parses");
        assert_eq!(
            switched.view_chips().unwrap().equipped_folder_index(),
            1,
            "the equipped-deck byte is what the view reports"
        );

        let model = tango_gamesupport_common::model::from_patched_rom(
            game,
            rom,
            std::path::PathBuf::new(),
            Box::new(switched),
            None,
        );
        let mut loaded = tango_gamesupport_common::editor::loaded::from_model(model, &SAVE_EDITOR.0, &[game]);
        let _ = apply_edit(
            &mut loaded.model,
            Edit::Chips(ChipEdit::SetChip { slot: 0, chip: star(1) }),
        );
        let text = SAVE_EDITOR
            .0
            .tab_as_text(Tab::ProgramDeck, &loaded, RenderOpts::default())
            .expect("deck as text");
        assert!(
            text.contains("1\tCannon"),
            "the edit landed in the equipped deck: {text:?}"
        );
        assert!(
            !text.contains("L\t"),
            "deck 1's board is its own — deck 0's L slot must not show: {text:?}"
        );
        assert!(
            text.contains("Navi\tMegaMan"),
            "deck 1 keeps its own navi, not deck 0's GutsMan: {text:?}"
        );
    }
}

/// Every chip name and description must decode to real glyphs: the
/// charsets are pinned to what the games actually draw, so a
/// replacement char here means a code the tables don't cover.
fn assert_no_undecoded(assets: &tango_gamesupport_bcc::dataview::rom::Assets, region: &str) {
    for id in 0..tango_gamesupport_bcc::dataview::NUM_CHIPS {
        let Some(chip) = assets.chip_info(id) else { continue };
        for (what, text) in [("name", chip.name()), ("description", chip.description())] {
            let Some(text) = text else { continue };
            assert!(
                !text.contains('\u{fffd}'),
                "{region} chip {id} {what} has an undecoded glyph: {text:?}"
            );
        }
    }
}
