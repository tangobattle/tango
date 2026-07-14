//! Throughput benchmark for the SIO-rollback pair, under the workloads a
//! netplay session actually imposes:
//!
//!   plain      — tick the pair
//!   +snapshot  — tick + full pair snapshot every tick (the session does
//!                this to keep its rollback window)
//!   +rollback  — additionally restore 8 ticks back and re-simulate every
//!                60 ticks (a pessimistic correction cadence)
//!
//! Usage: pair_bench [rom.gba [save.sav]]
//! Without arguments, runs the built-in SIO ping-pong test ROM.

use tango_siolink::{testrom, Pair, PairOptions, SideOptions};

const WARMUP: u32 = 120;
const TICKS: u32 = 600;
const ROLLBACK_DEPTH: usize = 8;
const ROLLBACK_EVERY: u32 = 60;

fn build_pair(rom: &[u8], save: Option<&[u8]>) -> Pair {
    Pair::with_options(PairOptions {
        sides: [
            SideOptions {
                rom: rom.to_vec(),
                save: save.map(|s| s.to_vec()),
            },
            SideOptions {
                rom: rom.to_vec(),
                save: save.map(|s| s.to_vec()),
            },
        ],
        rtc: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_752_000_000)),
    })
    .unwrap()
}

fn keys(t: u32) -> [u32; 2] {
    // Mash A/B/Start-ish so real games leave the title screen.
    [
        if (t / 30) % 2 == 0 { 1 } else { 8 },
        if (t / 37) % 2 == 0 { 2 } else { 8 },
    ]
}

fn bench(name: &str, rom: &[u8], save: Option<&[u8]>, mut f: impl FnMut(&mut Pair, u32)) {
    let mut pair = build_pair(rom, save);
    for t in 0..WARMUP {
        pair.tick(keys(t));
    }
    let start = std::time::Instant::now();
    for t in 0..TICKS {
        f(&mut pair, WARMUP + t);
    }
    let dt = start.elapsed();
    let tps = TICKS as f64 / dt.as_secs_f64();
    println!(
        "  {name:11} {tps:8.1} ticks/s   ({:5.1}x realtime, {:6.2} ms/tick)",
        tps / 59.7275,
        dt.as_secs_f64() * 1000.0 / TICKS as f64
    );
}

fn main() {
    mgba::log::install_default_logger();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let (label, rom, save) = match args.first() {
        Some(path) => (
            path.clone(),
            std::fs::read(path).expect("rom unreadable"),
            args.get(1).map(|p| std::fs::read(p).expect("save unreadable")),
        ),
        None => ("built-in SIO ping-pong test ROM".to_owned(), testrom::build(), None),
    };

    println!("{label}: {} ticks after {} warmup", TICKS, WARMUP);

    bench("plain", &rom, save.as_deref(), |pair, t| {
        pair.tick(keys(t));
    });

    bench("+snapshot", &rom, save.as_deref(), |pair, t| {
        let _ = pair.save().unwrap();
        pair.tick(keys(t));
    });

    let mut ring: std::collections::VecDeque<tango_siolink::Snapshot> = std::collections::VecDeque::new();
    bench("+rollback", &rom, save.as_deref(), move |pair, t| {
        if ring.len() > ROLLBACK_DEPTH {
            ring.pop_front();
        }
        ring.push_back(pair.save().unwrap());
        if t % ROLLBACK_EVERY == 0 && ring.len() > ROLLBACK_DEPTH {
            pair.load(&ring[0]).unwrap();
            for r in 0..ROLLBACK_DEPTH as u32 {
                pair.tick(keys(t - ROLLBACK_DEPTH as u32 + r));
            }
        }
        pair.tick(keys(t));
    });
}
