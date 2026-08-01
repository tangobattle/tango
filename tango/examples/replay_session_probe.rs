//! Drive real [`tango_session::ReplaySession`]s back to back on one
//! thread, the way the app's viewer closes one recording and opens the
//! next — workers, prefetch pass, stats job and all. The layers below
//! (ReplaySet, the engines) already have their own probes; this one
//! exists for whatever only the session layer does.
//!
//! Usage: replay_session_probe <replay.tangoreplay>:<rom> ...
//!
//! With --churn and exactly two entries, the first session keeps
//! playing on its own thread while the second boots, and is torn down
//! at a sweep of offsets into that boot — the app's shape when the
//! viewer swaps recordings, where the old session's teardown races the
//! new session's priming.

use std::sync::{Arc, Mutex};

fn game_of(family: &str, variant: u32) -> &'static tango_gamesupport::Game {
    tango_library::game::FAMILIES
        .iter()
        .find(|f| f.id == family)
        .unwrap_or_else(|| panic!("family {family:?} not in this build"))
        .games
        .iter()
        .find(|g| g.variant == variant as u8)
        .expect("variant")
}

struct Opened {
    session: tango_session::replay::ReplaySession,
    driver: tango_session::replay::Driver,
    _stream: tango_session::audio::Stream,
}

fn open_session(arg: &str, tag: &str) -> Opened {
    // "replay:rom:nostats" opens the way the app does when the match
    // stats are already cached — the second open of the same file.
    let (arg, with_stats) = match arg.strip_suffix(":nostats") {
        Some(rest) => (rest, false),
        None => (arg, true),
    };
    let (replay_path, rom_path) = arg.rsplit_once(':').expect("replay:rom");
    let f = std::fs::File::open(replay_path).expect("replay unreadable");
    let replay = Arc::new(tango_replay::Replay::decode(std::io::BufReader::new(f)).expect("replay undecodable"));
    let gi = replay
        .local_side()
        .and_then(|s| s.game_info.as_ref())
        .expect("replay carries no game info");
    let game = game_of(&gi.rom_family, gi.rom_variant);
    let rom = Arc::new(std::fs::read(rom_path).expect("rom unreadable"));
    let timing = game.pvp.frame_timing();
    let fps = timing.timescale as f32 / timing.frame_duration as f32;
    let (partial_tx, partial_rx) = futures::channel::mpsc::unbounded();
    std::mem::forget(partial_rx);
    let stats_job = with_stats.then(|| tango_session::replay::PrefetchStatsJob {
        partial_tx,
        done: Arc::new(Mutex::new(None)),
        stats_file: std::env::temp_dir().join(format!("replay_session_probe-{tag}.stats")),
    });
    let (session, workers, stream) = tango_session::replay::ReplaySession::new(
        [game, game],
        [rom.clone(), rom.clone()],
        replay,
        fps,
        48_000,
        true,
        stats_job,
        // Nothing analyzed up front: the probe wants the prefetch pass to
        // find the round boundaries itself.
        vec![],
    )
    .expect("session open");
    Opened {
        session,
        driver: workers.into_driver(),
        _stream: stream,
    }
}

fn churn(a_arg: &str, b_arg: &str) {
    use tango_session::Drive;
    for stagger_ms in [0u64, 500, 1000, 2000, 3000, 4000] {
        println!("=== churn: teardown of A at +{stagger_ms}ms into B's boot");
        let mut a = open_session(a_arg, "a");
        // A plays on its own thread until told to die; the teardown
        // (session drop, pairs and all) happens on that thread, like a
        // worker unwinding.
        let (die_tx, die_rx) = std::sync::mpsc::channel::<()>();
        let a_thread = std::thread::spawn(move || {
            while a.session.current_tick() < 400 && a.driver.tick() {
                a.driver.prefetch_step(64);
            }
            let _ = die_rx.recv();
            drop(a);
        });

        let (b_tx, b_rx) = std::sync::mpsc::channel::<()>();
        let killer = std::thread::spawn(move || {
            let _ = b_rx.recv();
            std::thread::sleep(std::time::Duration::from_millis(stagger_ms));
            let _ = die_tx.send(());
        });

        let mut b = open_session(b_arg, "b");
        let _ = b_tx.send(());
        let started = std::time::Instant::now();
        let mut alive = true;
        while alive && b.session.current_tick() < 400 && started.elapsed().as_secs() < 90 {
            for _ in 0..64 {
                if !b.driver.tick() {
                    alive = false;
                    break;
                }
            }
            b.driver.prefetch_step(64);
        }
        println!(
            "    B: tick {}/{} alive={alive} in {:.1?}",
            b.session.current_tick(),
            b.session.total_ticks(),
            started.elapsed()
        );
        a_thread.join().unwrap();
        killer.join().unwrap();
        if !alive || b.session.current_tick() < 400 {
            println!("REPRODUCED at +{stagger_ms}ms");
            return;
        }
    }
    println!("no reproduction across staggers");
}

/// Play A to its very end — through the recording's wireless teardown
/// and whatever the completion anchor does — leave the finished session
/// standing, and only then boot B. The app's shape when a replay is
/// watched to the end and the next one opened.
fn after(a_arg: &str, b_arg: &str) {
    use tango_session::Drive;
    println!("=== after: playing A to completion");
    let mut a = open_session(a_arg, "a");
    let started = std::time::Instant::now();
    while a.driver.tick() && started.elapsed().as_secs() < 240 {
        a.driver.prefetch_step(256);
    }
    while a.driver.prefetch_step(256) {}
    println!(
        "    A: tick {}/{} prefetch {} in {:.1?} (holding it open)",
        a.session.current_tick(),
        a.session.total_ticks(),
        a.session.prefetch_progress(),
        started.elapsed()
    );

    println!("=== after: booting B with A still standing");
    let mut b = open_session(b_arg, "b");
    let started = std::time::Instant::now();
    let mut alive = true;
    while alive && b.session.current_tick() < 400 && started.elapsed().as_secs() < 90 {
        for _ in 0..64 {
            if !b.driver.tick() {
                alive = false;
                break;
            }
        }
        b.driver.prefetch_step(64);
    }
    println!(
        "    B: tick {}/{} alive={alive} in {:.1?}",
        b.session.current_tick(),
        b.session.total_ticks(),
        started.elapsed()
    );
    drop(a);
    if !alive || b.session.current_tick() < 400 {
        println!("REPRODUCED");
    } else {
        println!("no reproduction");
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a == "--churn").unwrap_or(false) {
        churn(&args[1], &args[2]);
        return;
    }
    if args.first().map(|a| a == "--after").unwrap_or(false) {
        after(&args[1], &args[2]);
        return;
    }
    for (i, arg) in args.iter().enumerate() {
        println!("=== session {i}: {arg}");
        let mut s = open_session(arg, &i.to_string());
        let started = std::time::Instant::now();

        use tango_session::Drive;
        let mut alive = true;
        while alive && s.session.current_tick() < 400 && started.elapsed().as_secs() < 90 {
            for _ in 0..64 {
                if !s.driver.tick() {
                    alive = false;
                    break;
                }
            }
            s.driver.prefetch_step(64);
        }
        println!(
            "    session {i}: tick {}/{} prefetch {} alive={alive} in {:.1?}",
            s.session.current_tick(),
            s.session.total_ticks(),
            s.session.prefetch_progress(),
            started.elapsed()
        );
        assert!(alive, "session {i} died");
        assert!(s.session.current_tick() >= 400, "session {i} never reached tick 400");
        drop(s);
        println!("    session {i}: closed");
    }
    println!("SUCCESS");
}
