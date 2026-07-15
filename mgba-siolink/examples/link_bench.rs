//! Throughput benchmark for the SIO-rollback link, under the workloads a
//! netplay session actually imposes:
//!
//!   plain       — tick the link, all cores rendering
//!   remote-skip — tick with the remote cores' renderers off (the session
//!                 default: live play only presents the local side)
//!   no-render   — tick with all renderers off (re-simulation cost)
//!   +snapshot   — remote-skip + full link snapshot every tick (the
//!                 session does this to keep its rollback window)
//!   +rollback   — additionally restore 8 ticks back and re-simulate every
//!                 60 ticks (a pessimistic correction cadence), rendering
//!                 nothing during the re-sim
//!
//! Usage: link_bench [players] [rom.gba [save.sav]]
//! Without a ROM, runs the built-in SIO ping-pong test ROM; `players`
//! (default 2) sizes the link. Note real games only link up in their own
//! 2-player modes; 3-4 player links are exercised by the test ROM.

use mgba_siolink::{testrom, Link, LinkOptions, SideOptions};

const WARMUP: u32 = 120;
const TICKS: u32 = 600;
const ROLLBACK_DEPTH: usize = 8;
const ROLLBACK_EVERY: u32 = 60;

fn build_link(num_players: usize, rom: &[u8], save: Option<&[u8]>) -> Link {
    Link::with_options(LinkOptions {
        sides: (0..num_players)
            .map(|_| SideOptions {
                rom: rom.to_vec(),
                save: save.map(|s| s.to_vec()),
            })
            .collect(),
        rtc: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_752_000_000)),
    })
    .unwrap()
}

fn keys(t: u32, num_players: usize) -> Vec<u32> {
    // Mash A/B/Start-ish so real games leave the title screen.
    (0..num_players as u32)
        .map(|p| if (t / (30 + 7 * p)) % 2 == 0 { 1 << (p % 2) } else { 8 })
        .collect()
}

fn bench(
    name: &str,
    num_players: usize,
    rom: &[u8],
    save: Option<&[u8]>,
    setup: impl FnOnce(&mut Link),
    mut f: impl FnMut(&mut Link, u32),
) {
    let mut link = build_link(num_players, rom, save);
    for t in 0..WARMUP {
        link.tick(&keys(t, num_players));
    }
    setup(&mut link);
    let start = std::time::Instant::now();
    for t in 0..TICKS {
        f(&mut link, WARMUP + t);
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

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let num_players = match args.first().and_then(|a| a.parse::<usize>().ok()) {
        Some(n) => {
            args.remove(0);
            n
        }
        None => 2,
    };
    let (label, rom, save) = match args.first() {
        Some(path) => (
            path.clone(),
            std::fs::read(path).expect("rom unreadable"),
            args.get(1).map(|p| std::fs::read(p).expect("save unreadable")),
        ),
        None => ("built-in SIO ping-pong test ROM".to_owned(), testrom::build(), None),
    };

    println!("{label} ({num_players} players): {TICKS} ticks after {WARMUP} warmup");

    let skip_remotes = |link: &mut Link| {
        for i in 1..num_players {
            link.set_frameskip(i, i32::MAX);
        }
    };

    bench(
        "plain",
        num_players,
        &rom,
        save.as_deref(),
        |_| {},
        |link, t| {
            link.tick(&keys(t, num_players));
        },
    );

    bench(
        "remote-skip",
        num_players,
        &rom,
        save.as_deref(),
        skip_remotes,
        |link, t| {
            link.tick(&keys(t, num_players));
        },
    );

    bench(
        "no-render",
        num_players,
        &rom,
        save.as_deref(),
        |link| {
            for i in 0..num_players {
                link.set_frameskip(i, i32::MAX);
            }
        },
        |link, t| {
            link.tick(&keys(t, num_players));
        },
    );

    bench(
        "+snapshot",
        num_players,
        &rom,
        save.as_deref(),
        skip_remotes,
        |link, t| {
            let _ = link.save().unwrap();
            link.tick(&keys(t, num_players));
        },
    );

    let mut ring: std::collections::VecDeque<mgba_siolink::Snapshot> = std::collections::VecDeque::new();
    bench(
        "+rollback",
        num_players,
        &rom,
        save.as_deref(),
        skip_remotes,
        move |link, t| {
            if ring.len() > ROLLBACK_DEPTH {
                ring.pop_front();
            }
            ring.push_back(link.save().unwrap());
            if t % ROLLBACK_EVERY == 0 && ring.len() > ROLLBACK_DEPTH {
                link.set_frameskip(0, i32::MAX);
                link.load(&ring[0]).unwrap();
                for r in 0..ROLLBACK_DEPTH as u32 {
                    link.tick(&keys(t - ROLLBACK_DEPTH as u32 + r, num_players));
                }
                link.set_frameskip(0, 0);
            }
            link.tick(&keys(t, num_players));
        },
    );
}
