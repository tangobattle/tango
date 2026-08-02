//! Headless check of the save layer: parse a cartridge dump, print what
//! every view reads out of each in-game file, exercise an edit, and
//! confirm the result re-parses into a block the game would load.
//!
//! Usage: save_probe <rom> <sram> [<out_sram>]
//!
//! With `out_sram`, the played file gets an edit written there so it can
//! be booted in the real game (`menu_probe <rom> --save <out_sram>
//! --prime --frames 500 --shot-at 499`) and the result compared against
//! what this printed. The edit is a NaviCust one — an HP+500 program
//! dropped on the grid — because the battle's own HP counter is then
//! the game's answer to whether the mapping is right.

use tango_gamesupport_bn5ds::dataview::{rom, save};
use tango_gamesupport_common::dataview::rom::Assets as _;
use tango_gamesupport_common::dataview::save::Save as _;

/// The HP+500 program, in its first colour: template 47 of the cart's
/// own table, four colour variants apiece.
const HP_PLUS_500: usize = 47 * 4;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom_data = std::fs::read(&args[0]).expect("rom unreadable");
    let sram = std::fs::read(&args[1]).expect("sram unreadable");
    let out_path = args.get(2);

    let offsets = match &rom_data[0xc..0x10] {
        b"A5TE" => &rom::A5TE_00,
        b"A5TJ" => &rom::A5TJ_00,
        code => panic!("not a bn5ds cart (code {code:02x?})"),
    };
    let charset: &[&str] = if offsets as *const _ == &rom::A5TE_00 as *const _ {
        rom::EN_CHARSET
    } else {
        rom::JA_CHARSET
    };
    let assets = rom::Assets::new(offsets, charset, rom_data);

    if args.iter().any(|arg| arg == "--parts") {
        println!("navicust programs:");
        for id in 0..tango_gamesupport_bn5ds::dataview::NUM_NAVICUST_PARTS {
            let Some(part) = assets.navicust_part(id) else { continue };
            println!(
                "  {id:3} (template {:2}) {:10} {:?}{:7} {}",
                id / 4,
                part.name().unwrap_or_default(),
                part.color(),
                if part.is_solid() { " solid" } else { "" },
                part.description().unwrap_or_default().replace('\n', " / "),
            );
        }
    }

    let set = save::SaveSet::parse(&sram).expect("save did not parse");
    println!("cart holds files {:?}, current is {}", set.slots(), set.current().slot());

    for slot in set.slots() {
        let file = set.save(slot).expect("the slot came from this set");
        println!("\n=== file {slot} (team {}, cross {:?}) ===", file.team(), file.cross());
        report(&file, &assets);
    }

    // The edit goes to the file the cartridge plays, which is what a
    // session and the priming walk boot.
    let mut file = set.current();
    let navi_hp_before = file.view_navi().unwrap().max_hp(&assets);
    {
        let mut navicust = file.view_navicust_mut().expect("navicust is not editable");
        // The first free slot, dropped on a free corner of the grid.
        let slot = (0..25)
            .find(|&i| navicust.navicust_part(i).is_none())
            .expect("navicust is full");
        assert!(navicust.set_navicust_part(
            slot,
            Some(tango_gamesupport_common::dataview::save::NavicustPart {
                id: HP_PLUS_500,
                col: 0,
                row: 0,
                rot: 0,
                compressed: false,
            })
        ));
        navicust.rebuild_materialized(&assets);
    }
    // And a folder edit, which is the other write path a session cares
    // about: the equipped folder's first two chips change places. A
    // reorder keeps the folder legal whatever the cart owns, so a boot
    // that fails after this one is a real failure rather than the game
    // quietly dropping a chip the pack hasn't got.
    {
        let mut chips = file.view_chips_mut().expect("the folder is not editable");
        let folder = chips.equipped_folder_index();
        let (first, second) = (chips.chip(folder, 0), chips.chip(folder, 1));
        if let (Some(first), Some(second)) = (first, second) {
            assert!(chips.set_chip(folder, 0, second));
            assert!(chips.set_chip(folder, 1, first));
        }
    }
    file.rebuild_checksum();
    let navi_hp_after = file.view_navi().unwrap().max_hp(&assets);
    println!("\nadded HP+500 to file {}: max HP {navi_hp_before} -> {navi_hp_after}", file.slot());
    println!("swapped the equipped folder's first two chips");

    let dump = file.to_sram_dump();
    let reparsed = save::SaveSet::parse(&dump).expect("edited dump did not re-parse");
    let reparsed = reparsed.current();
    assert_eq!(reparsed.slot(), file.slot(), "the edit changed which file is current");
    assert_eq!(
        reparsed.view_navi().unwrap().max_hp(&assets),
        navi_hp_after,
        "the edit did not survive the round trip"
    );
    println!("re-parsed: same file, same HP");

    if let Some(out) = out_path {
        std::fs::write(out, &dump).expect("could not write");
        println!("wrote {out}");
    }
}

fn report(file: &save::Save, assets: &rom::Assets) {
    let navi = file.view_navi().unwrap();
    let limits = navi.folder_limits(assets);
    println!(
        "navi {}: {} HP; mega {:?} giga {:?} dark {:?} reg {:?} MB",
        navi.navi(),
        navi.max_hp(assets),
        limits.mega_limit,
        limits.giga_limit,
        limits.dark_limit,
        limits.reg_memory,
    );

    if let Some(navicust) = file.view_navicust() {
        let name = |id: usize| {
            assets
                .navicust_part(id)
                .and_then(|part| tango_gamesupport_common::dataview::rom::NavicustPart::name(&*part))
                .unwrap_or_else(|| "???".to_string())
        };
        println!("navicust:");
        for slot in 0..navicust.count() {
            let Some(part) = navicust.navicust_part(slot) else { continue };
            println!(
                "  {slot:2}: {:12} id {:3} at ({}, {}) rot {} {}",
                name(part.id),
                part.id,
                part.col,
                part.row,
                part.rot,
                if part.compressed { "compressed" } else { "" }
            );
        }
        let grid = navicust.materialized();
        for row in grid.rows() {
            println!(
                "       [{}]",
                row.iter()
                    .map(|cell| cell.map(|slot| format!("{slot:2}")).unwrap_or_else(|| " .".to_string()))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        println!(
            "  colour bar: {:?}",
            navicust.navicust_color_bar().into_iter().flatten().collect::<Vec<_>>()
        );
    }

    let chips = file.view_chips().unwrap();
    let equipped = chips.equipped_folder_index();
    let filled = (0..chips.folder_size())
        .filter(|&i| chips.chip(equipped, i).is_some())
        .count();
    println!(
        "folder {equipped} of {}: {filled}/{} chips, regular {:?}",
        chips.num_folders(),
        chips.folder_size(),
        chips.regular_chip_index(equipped).flatten(),
    );

    let abd = file.view_auto_battle_data().unwrap();
    let named = |chip: Option<usize>| {
        chip.map(|id| {
            assets
                .chip(id)
                .and_then(|c| tango_gamesupport_common::dataview::rom::Chip::name(&*c))
                .unwrap_or_else(|| format!("#{id}"))
        })
        .unwrap_or_else(|| "-".to_string())
    };
    let materialized = abd.materialized();
    println!(
        "auto battle: standard {:?}",
        materialized.standard_chips().iter().map(|c| named(*c)).collect::<Vec<_>>()
    );
    println!(
        "             mega {:?} giga {}",
        materialized.mega_chips().iter().map(|c| named(*c)).collect::<Vec<_>>(),
        named(materialized.giga_chip()),
    );
    let used = (0..tango_gamesupport_bn5ds::dataview::NUM_AUTO_BATTLE_DATA_CHIPS)
        .filter(|&id| abd.chip_use_count(id).unwrap_or(0) > 0)
        .count();
    println!("             {used} chips have a use count");
}
