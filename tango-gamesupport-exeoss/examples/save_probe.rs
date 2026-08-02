//! Headless check of the save layer: parse a cartridge dump, print the
//! bank it opened on and the folder it holds, exercise an edit, and
//! confirm the result re-parses with the checksums the console checks.
//!
//! Usage: save_probe <rom> <sram> [<out_sram>]
//!
//! With `out_sram`, the edited save is written there so it can be
//! booted in the real game (`menu_probe <rom> --save <out_sram>
//! --walk`) and the folder the game plays compared against what this
//! printed.
//!
//! `--pack` additionally prints every chip the pack holds, beside the
//! codes the ROM lists for it — the cross-check that pins the pack
//! row's shape.

use tango_gamesupport_common::dataview::rom::Assets as _;
use tango_gamesupport_common::dataview::save::Save as _;
use tango_gamesupport_exeoss::dataview::{rom, save};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom_path = &args[0];
    let sram_path = &args[1];
    let out_path = args.get(2).filter(|arg| !arg.starts_with("--"));
    let show_pack = args.iter().any(|arg| arg == "--pack");

    let rom_data = std::fs::read(rom_path).expect("rom unreadable");
    let sram = std::fs::read(sram_path).expect("sram unreadable");

    let assets = rom::Assets::new(&rom::B6XJ_00, rom::JA_CHARSET, rom_data);
    let name = |id: usize| {
        assets
            .chip(id)
            .and_then(|chip| tango_gamesupport_common::dataview::rom::Chip::name(&*chip))
            .unwrap_or_else(|| "???".to_string())
    };
    let codes = |id: usize| {
        assets
            .chip(id)
            .map(|chip| tango_gamesupport_common::dataview::rom::Chip::codes(&*chip))
            .unwrap_or_default()
    };

    let mut save = save::Save::new(&sram).expect("save did not parse");
    println!("parsed {} bytes, generation {}", sram.len(), save.generation());

    let navi = save.view_navi().expect("no navi view");
    let limits = navi.folder_limits(&assets);
    println!(
        "navi: {} HP, up to {:?} navi chips and {} copies of a chip",
        navi.max_hp(&assets),
        limits.navi_limit,
        assets
            .chip(1)
            .map(|chip| (limits.max_copies)(chip.as_ref()))
            .unwrap_or_default(),
    );
    drop(navi);

    let print_folder = |save: &save::Save| {
        let view = save.view_chips().unwrap();
        for slot in 0..view.folder_size() {
            match view.chip(0, slot) {
                Some(chip) => println!("  {slot:2}: {:3} {} {}", chip.id, chip.code, name(chip.id)),
                None => println!("  {slot:2}: -"),
            }
        }
    };
    println!("folder:");
    print_folder(&save);

    if show_pack {
        println!("pack:");
        let view = save.view_chips().unwrap();
        for id in 0..tango_gamesupport_exeoss::dataview::NUM_CHIPS {
            let counts: Vec<String> = codes(id)
                .into_iter()
                .enumerate()
                .filter_map(|(variant, code)| {
                    let count = view.pack_count(id, variant)?;
                    (count > 0).then(|| format!("{code}x{count}"))
                })
                .collect();
            if !counts.is_empty() {
                println!("  {id:3} {:12} {}", name(id), counts.join(" "));
            }
        }
    }

    // The edit: fill the folder with one chip in one code, which is
    // unmistakable on the game's own folder and chip-select screens.
    // Cannon A — id 1, the first code the ROM lists for it.
    //
    // Deliberately past the cart's own five-copies rule: that rule is
    // the editor's to enforce, and the view writes what it is told, so
    // a wall of one chip is both a fair test of the write path and the
    // easiest thing to recognize on a screenshot.
    let chip = tango_gamesupport_common::dataview::save::Chip {
        id: 1,
        code: tango_gamesupport_common::dataview::save::ChipCode::A,
    };
    {
        let mut view = save.view_chips_mut().expect("folder is not editable");
        for slot in 0..30 {
            assert!(view.set_chip(0, slot, chip.clone()), "slot {slot} refused");
        }
    }
    save.rebuild_checksum();
    println!("edited folder to 30x {} {}:", name(chip.id), chip.code);
    print_folder(&save);

    let dump = save.to_sram_dump();
    assert_eq!(dump.len(), save::SIZE, "a dump is the whole flash");
    let reparsed = save::Save::new(&dump).expect("edited save did not re-parse");
    assert_eq!(reparsed.generation(), save.generation(), "the edit changed banks");
    let view = reparsed.view_chips().unwrap();
    for slot in 0..30 {
        assert_eq!(view.chip(0, slot).as_ref(), Some(&chip), "slot {slot} came back wrong");
    }
    println!("re-parsed: same bank, same folder");

    if let Some(out) = out_path {
        std::fs::write(out, &dump).expect("could not write");
        println!("wrote {out}");
    }
}
