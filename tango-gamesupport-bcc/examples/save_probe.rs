//! Headless check of the save layer: parse an SRAM dump, print the
//! decks it holds, exercise an edit, and confirm the result re-parses.
//!
//! Usage: save_probe <rom> <sram> [<out_sram>]
//!
//! With `out_sram`, the edited save is written there so it can be booted
//! in the real game and the result compared against what this printed.
//!
//! `BCC_ART_DIR=<dir>` additionally dumps every chip's artwork and icon
//! as PNGs, for eyeballing the ROM graphics decode.

use tango_gamesupport_common_dataview::rom::Assets as _;
use tango_gamesupport_common_dataview::save::Save as _;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("rom unreadable");
    let sram = std::fs::read(&args[1]).expect("sram unreadable");

    let offsets = match &rom[0xac..0xb0] {
        b"A89E" => &tango_gamesupport_bcc::dataview::rom::A89E_00,
        b"A89J" => &tango_gamesupport_bcc::dataview::rom::A89J_00,
        code => panic!("not a bcc rom (code {:02x?})", code),
    };
    let assets = tango_gamesupport_bcc::dataview::rom::Assets::new(
        offsets,
        tango_gamesupport_bcc::dataview::rom::EN_CHARSET,
        rom.clone(),
        vec![0; 0x40000],
    );
    let name = |id: usize| assets.chip(id).and_then(|c| c.name()).unwrap_or_default();

    if let Ok(dir) = std::env::var("BCC_ART_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        for id in 1..tango_gamesupport_bcc::dataview::NUM_CHIPS {
            let Some(chip) = assets.chip(id) else { continue };
            let nm = tango_gamesupport_common_dataview::rom::Chip::name(&*chip)
                .unwrap_or_default()
                .replace(['/', ' '], "_");
            tango_gamesupport_common_dataview::rom::Chip::image(&*chip)
                .save(format!("{dir}/{id:03}_{nm}.png"))
                .unwrap();
            tango_gamesupport_common_dataview::rom::Chip::icon(&*chip)
                .save(format!("{dir}/{id:03}_{nm}_icon.png"))
                .unwrap();
        }
        println!("wrote artwork to {dir}");
    }

    let mut save = tango_gamesupport_bcc::dataview::save::Save::new(&sram).expect("save did not parse");

    {
        let view = save.view_chips().unwrap();
        println!("decks: {}  slots/deck: {}", view.num_folders(), view.folder_size());
        for deck in 0..view.num_folders() {
            let navi = match view.chip(deck, tango_gamesupport_bcc::dataview::save::NAVI_SLOT) {
                Some(c) => format!("{} (id {})", name(c.id), c.id),
                None => "-".to_string(),
            };
            let equipped: Vec<String> = (0..view.folder_size())
                .map(|s| match view.chip(deck, s) {
                    Some(c) => format!("{} (id {}, {} MB)", name(c.id), c.id, assets.chip(c.id).unwrap().mb()),
                    None => "-".to_string(),
                })
                .collect();
            println!("  deck {deck}: navi {navi} — {}", equipped.join(", "));
        }
        let owned: Vec<String> = (1..tango_gamesupport_bcc::dataview::NUM_CHIPS)
            .filter_map(|id| {
                let n = view.pack_count(id, 0)?;
                (n > 0).then(|| format!("{}×{}", name(id), n))
            })
            .collect();
        println!("  folder: {}", owned.join(", "));
    }

    // Edit: equip a chip the folder owns into the first empty slot, and
    // clear the first occupied one — then check both land. A deck with
    // no empty slot (or nothing equipped) skips the exercise: this is a
    // probe over arbitrary saves, not a fixture.
    let before = save.to_sram_dump();
    let slots = {
        let view = save.view_chips().unwrap();
        let empty = (0..view.folder_size()).find(|&s| view.chip(0, s).is_none());
        let filled = (0..view.folder_size()).find(|&s| view.chip(0, s).is_some());
        let id = (1..tango_gamesupport_bcc::dataview::NUM_CHIPS)
            .find(|&id| view.pack_count(id, 0).is_some_and(|n| n > 0));
        empty.zip(filled).zip(id)
    };
    let Some(((empty_slot, filled_slot), owned_id)) = slots else {
        println!("deck 0 has no empty+filled slot pair; skipping the edit exercise");
        return;
    };
    {
        let mut view = save.view_chips_mut().unwrap();
        assert!(
            view.set_chip(
                0,
                empty_slot,
                tango_gamesupport_common_dataview::save::Chip {
                    id: owned_id,
                    code: tango_gamesupport_common_dataview::save::ChipCode::Star,
                },
            ),
            "set_chip refused"
        );
        assert!(view.clear_chip(0, filled_slot), "clear_chip refused");
    }
    {
        let view = save.view_chips().unwrap();
        assert_eq!(
            view.chip(0, empty_slot).map(|c| c.id),
            Some(owned_id),
            "equip did not take"
        );
        assert!(view.chip(0, filled_slot).is_none(), "clear did not take");
    }
    println!(
        "edit: equipped {} in slot {empty_slot}, cleared slot {filled_slot}",
        name(owned_id)
    );

    // A reorder rewrites the deck slot by slot, asking for chips that are
    // still equipped further down — the folder must end up with the same
    // copies, just rebound, never duplicated.
    {
        let owned_before: Vec<(usize, usize)> = {
            let view = save.view_chips().unwrap();
            (1..tango_gamesupport_bcc::dataview::NUM_CHIPS)
                .filter_map(|id| Some((id, view.pack_count(id, 0)?)))
                .filter(|&(_, n)| n > 0)
                .collect()
        };
        let equipped: Vec<Option<usize>> = {
            let view = save.view_chips().unwrap();
            (0..view.folder_size()).map(|s| view.chip(0, s).map(|c| c.id)).collect()
        };
        let mut reversed = equipped.clone();
        reversed.reverse();
        {
            let mut view = save.view_chips_mut().unwrap();
            for (slot, id) in reversed.iter().enumerate() {
                match id {
                    Some(id) => {
                        view.set_chip(
                            0,
                            slot,
                            tango_gamesupport_common_dataview::save::Chip {
                                id: *id,
                                code: tango_gamesupport_common_dataview::save::ChipCode::Star,
                            },
                        );
                    }
                    None => {
                        view.clear_chip(0, slot);
                    }
                }
            }
        }
        let view = save.view_chips().unwrap();
        let now: Vec<Option<usize>> = (0..view.folder_size()).map(|s| view.chip(0, s).map(|c| c.id)).collect();
        assert_eq!(now, reversed, "reorder did not land");
        let owned_after: Vec<(usize, usize)> = (1..tango_gamesupport_bcc::dataview::NUM_CHIPS)
            .filter_map(|id| Some((id, view.pack_count(id, 0)?)))
            .filter(|&(_, n)| n > 0)
            .collect();
        assert_eq!(owned_after, owned_before, "reorder changed what the folder owns");
        println!("reorder: ok, folder unchanged");
    }

    // The edited save must survive a container round trip.
    let after = save.to_sram_dump();
    assert_ne!(before, after, "edit did not reach the sram dump");
    let reparsed = tango_gamesupport_bcc::dataview::save::Save::new(&after).expect("edited save did not re-parse");
    assert_eq!(
        reparsed.as_raw_wram(),
        save.as_raw_wram(),
        "round trip changed the payload"
    );
    println!("round trip: ok ({} bytes)", after.len());

    if let Some(out) = args.get(2) {
        // Equip everything the folder owns, so the game's own deck screen
        // shows whether the edits are laid out the way it expects.
        {
            let mut view = save.view_chips_mut().unwrap();
            let mut slot = 0;
            for id in 1..tango_gamesupport_bcc::dataview::NUM_CHIPS {
                for _ in 0..view.pack_count(id, 0).unwrap_or(0) {
                    if slot >= view.folder_size() {
                        break;
                    }
                    view.set_chip(
                        0,
                        slot,
                        tango_gamesupport_common_dataview::save::Chip {
                            id,
                            code: tango_gamesupport_common_dataview::save::ChipCode::Star,
                        },
                    );
                    slot += 1;
                }
            }
            println!("wrote {out}: equipped {slot} chips into deck 0");
        }
        std::fs::write(out, save.to_sram_dump()).unwrap();
    }
}
