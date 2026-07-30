//! Dump what a replay hands the priming walk: the decoded metadata and
//! the shape of the saves. For telling apart recordings that walk fine
//! from ones that stall, without booting anything.
//!
//! Usage: replay_meta [--sram-out PREFIX] <replay.tangoreplay>...
//!
//! With --sram-out, each replay's two saves are written out as
//! PREFIX-p<n>.sav for feeding back into menu_probe.

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let sram_out = args
        .iter()
        .position(|a| a == "--sram-out")
        .map(|i| {
            args.remove(i);
            args.remove(i)
        });
    for path in args {
        let f = std::fs::File::open(&path).expect("replay unreadable");
        let replay = tango_replay::Replay::decode(std::io::BufReader::new(f)).expect("replay undecodable");
        let m = &replay.metadata;
        let gi = m
            .side(replay.local_player_index)
            .and_then(|s| s.game_info.as_ref());
        println!(
            "{path}\n  ts={} ({:?})\n  match_type=({}, {}) local_player={} ticks={} complete={}\n  family={:?} replay_version={:?}\n  sram sizes=[{}, {}] sram crc32=[{:08x}, {:08x}] session_payloads={:?}",
            m.ts,
            replay.rtc_time(),
            m.match_type,
            m.match_subtype,
            replay.local_player_index,
            replay.inputs.len(),
            replay.is_complete,
            gi.map(|g| g.rom_family.as_str()),
            gi.map(|g| g.replay_version),
            replay.srams[0].len(),
            replay.srams[1].len(),
            crc32(&replay.srams[0]),
            crc32(&replay.srams[1]),
            replay.session_payloads(),
        );
        for (tick, row) in replay
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, row)| row.iter().any(|i| i.keys != 0 || i.touch.is_some()))
            .take(8)
        {
            println!(
                "  first inputs: tick {tick}: p1 keys={:03x} touch={:?} / p2 keys={:03x} touch={:?}",
                row[0].keys, row[0].touch, row[1].keys, row[1].touch
            );
        }
        if let Some(prefix) = &sram_out {
            for (i, sram) in replay.srams.iter().enumerate() {
                let out = format!("{prefix}-p{}.sav", i + 1);
                std::fs::write(&out, sram).expect("sram unwritable");
                println!("  wrote {out}");
            }
        }
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i as usize] = c;
    }
    !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}
