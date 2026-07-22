//! Print a replay's metadata and stream shape: schema, sides (family /
//! variant / patch), match type, completeness, and the round structure
//! the ROUND_START markers encode.
//!
//! Usage: replay_inspect <path.tangoreplay>...

fn main() {
    for path in std::env::args().skip(1) {
        println!("=== {path}");
        let f = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                println!("  open failed: {e}");
                continue;
            }
        };
        let replay = match tango_match::replay::Replay::decode(f) {
            Ok(r) => r,
            Err(e) => {
                println!("  decode failed: {e}");
                continue;
            }
        };
        let m = &replay.metadata;
        for (tag, side) in [("local", &m.local_side), ("remote", &m.remote_side)] {
            if let Some(side) = side {
                let gi = side.game_info.as_ref();
                println!(
                    "  {tag}: {} game={}/{} patch={:?}",
                    side.nickname,
                    gi.map(|g| g.rom_family.as_str()).unwrap_or("?"),
                    gi.map(|g| g.rom_variant).unwrap_or(0),
                    gi.and_then(|g| g.patch.as_ref())
                        .map(|p| format!("{} v{}", p.name, p.version)),
                );
            }
        }
        println!(
            "  ts={} match_type=({}, {}) local_player={} complete={}",
            m.ts, m.match_type, m.match_subtype, replay.local_player_index, replay.is_complete
        );
        println!(
            "  rounds={} lens={:?} total_inputs={}",
            replay.round_starts.len(),
            replay.round_ranges().map(|r| r.len()).collect::<Vec<_>>(),
            replay.inputs.len(),
        );
    }
}
