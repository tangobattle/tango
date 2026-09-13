//! Game-neutral instruction hooks over a linked GBA pair. Game addresses and
//! boot policy come exclusively from the supplied programs.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tango_match::gamemode as script;
mod session;
pub use session::Backend;

/// The host supplies this when preparing or resolving a package gamemode.
/// The upper half tracks emulation; the lower half tracks this hook adapter's
/// simulation semantics. Bump the latter when the same script runs differently.
pub fn runtime() -> script::identity::Runtime {
    script::identity::Runtime {
        name: "mgba".into(),
        revision: ((crate::backend::BACKEND_SIM_VERSION as u32) << 16) | 2,
    }
}
use tango_match::telemetry::{EventSink, Outcome, Telemetry};

fn error(error: impl std::fmt::Display) -> crate::Error {
    crate::Error::GameMode(error.to_string())
}

/// Instruction traps execute after the original Thumb instruction. Limit PCs
/// to the loaded ROM: mGBA's prefetch pointer requires mapped executable bytes.
fn thumb_pc(address: u32, rom_len: usize) -> bool {
    address % 2 == 0 && address >= 0x08000000 && u64::from(address) + 4 <= 0x08000000 + rom_len as u64
}

struct Machine<'a> {
    core: &'a mut mgba::core::Core,
    rom_len: usize,
    event_budget: usize,
}
impl script::Memory for Machine<'_> {
    fn read(&mut self, space: &str, address: u32, output: &mut [u8]) -> Result<(), script::Error> {
        if space != "main" || u64::from(address) + output.len() as u64 > u64::from(u32::MAX) + 1 {
            return Err("invalid GBA memory read".into());
        }
        self.core.raw_read_range(address, -1, output);
        Ok(())
    }
}
fn gpr(name: &str) -> Option<usize> {
    let index = name.strip_prefix('r')?.parse::<usize>().ok()?;
    (index < 15 && name == format!("r{index}")).then_some(index)
}
impl script::Reader for Machine<'_> {
    fn read_pending(
        &mut self,
        space: &str,
        address: u32,
        output: &mut [u8],
        writes: &[script::Write],
    ) -> Result<(), script::Error> {
        script::Memory::read(self, space, address, output)?;
        let mut offset = 0;
        while offset < output.len() {
            let address = address + offset as u32;
            let (canonical, remaining) = match address >> 24 {
                2 => (0x02000000 | (address & 0x3ffff), 0x40000 - (address & 0x3ffff)),
                3 => (0x03000000 | (address & 0x7fff), 0x8000 - (address & 0x7fff)),
                _ => (address, 0x1000000 - (address & 0xffffff)),
            };
            let length = (remaining as usize).min(output.len() - offset);
            script::overlay_writes(space, canonical, &mut output[offset..offset + length], writes);
            offset += length;
        }
        Ok(())
    }

    fn read_register(&mut self, name: &str) -> Result<u32, script::Error> {
        let cpu = self.core.gba().cpu();
        match name {
            "thumb_pc" => Ok(cpu.thumb_pc()),
            "cpsr" => Ok(cpu.cpsr() as u32),
            _ => gpr(name)
                .map(|index| cpu.gpr(index) as u32)
                .ok_or_else(|| "unknown GBA register".into()),
        }
    }
}
impl script::Machine for Machine<'_> {
    fn validate_update(&self, update: &script::Update) -> Result<(), script::Error> {
        if update.events.len() > self.event_budget {
            return Err("lifecycle event limit exceeded".into());
        }
        for write in &update.writes {
            match write {
                script::Write::Memory { space, address, data } => {
                    // RAM writes are raw, side-effect-free pokes. ROM edits
                    // belong in patch_rom; register/IO bus effects need a
                    // separate checked capability before they can be exposed.
                    let end = u64::from(*address) + data.len() as u64;
                    let ram =
                        (*address >= 0x02000000 && end <= 0x02040000) || (*address >= 0x03000000 && end <= 0x03008000);
                    if space != "main" || !ram {
                        return Err("GBA writes must stay in work RAM".into());
                    }
                }
                script::Write::Register { name, value } if name == "thumb_pc" => {
                    if !matches!(
                        self.core.gba().cpu().execution_mode(),
                        mgba::arm_core::ExecutionMode::Thumb
                    ) || !thumb_pc(*value, self.rom_len)
                    {
                        return Err("invalid GBA Thumb program counter".into());
                    }
                }
                script::Write::Register { name, .. } if gpr(name).is_some() => {}
                _ => return Err("unknown or read-only GBA register".into()),
            }
        }
        Ok(())
    }
    fn apply_writes(&mut self, writes: &[script::Write]) {
        for write in writes {
            match write {
                script::Write::Memory { address, data, .. } => self.core.raw_write_range(*address, -1, data),
                script::Write::Register { name, value } if name == "thumb_pc" => {
                    self.core.gba_mut().cpu_mut().set_thumb_pc(*value)
                }
                script::Write::Register { name, value } => {
                    self.core.gba_mut().cpu_mut().set_gpr(gpr(name).unwrap(), *value as i32)
                }
            }
        }
    }
}

struct Seat {
    program: script::Controller,
    ready: bool,
    failure: Option<String>,
    calls: usize,
    pending: Vec<script::Event>,
}
pub(crate) struct Pair {
    seats: [Arc<Mutex<Seat>>; 2],
    events: EventSink,
    priming: bool,
}
pub(crate) struct Snapshot {
    seats: [(script::Snapshot, bool); 2],
}

impl Pair {
    fn install(
        pair: &mut mgba_rollback::Link,
        programs: [Box<dyn script::Program>; 2],
        rom_lengths: [usize; 2],
        events: EventSink,
    ) -> Result<Self, crate::Error> {
        let [p0, p1] = programs;
        let programs = [
            script::Controller::new(p0).map_err(error)?,
            script::Controller::new(p1).map_err(error)?,
        ];
        // Validate both sides before patching either ROM with instruction traps.
        for (program, rom_len) in programs.iter().zip(rom_lengths) {
            if program.hooks().iter().any(|hook| !thumb_pc(hook.address, rom_len)) {
                return Err(error("GBA hooks require aligned Thumb addresses inside the loaded ROM"));
            }
        }
        let seats = programs.map(|program| {
            Arc::new(Mutex::new(Seat {
                program,
                ready: false,
                failure: None,
                calls: 4096,
                pending: Vec::new(),
            }))
        });
        for player in 0..2 {
            let hooks = seats[player].lock().unwrap().program.hooks().to_vec();
            let traps = hooks
                .into_iter()
                .map(|hook| {
                    let seat = seats[player].clone();
                    let rom_len = rom_lengths[player];
                    (
                        hook.address,
                        Box::new(move |core: &mut mgba::core::Core| {
                            let mut seat = seat.lock().unwrap();
                            if seat.failure.is_some() {
                                return;
                            }
                            if seat.calls == 0 {
                                seat.failure = Some("hook call limit exceeded in one frame".into());
                                return;
                            }
                            seat.calls -= 1;
                            let event_budget = 4096 - seat.pending.len();
                            match seat.program.on_hook(
                                &hook.name,
                                &mut Machine {
                                    core,
                                    rom_len,
                                    event_budget,
                                },
                            ) {
                                // The whole update, including the remaining event budget, passed validation.
                                Ok(events) => seat.pending.extend(events),
                                Err(e) => seat.failure = Some(format!("{}: {e}", hook.name)),
                            }
                        }) as Box<dyn Fn(&mut mgba::core::Core)>,
                    )
                })
                .collect();
            pair.set_traps(player, traps);
        }
        Ok(Self {
            seats,
            events,
            priming: true,
        })
    }
    pub(crate) fn check(&self) -> Result<(), crate::Error> {
        for (player, seat) in self.seats.iter().enumerate() {
            if let Some(failure) = &seat.lock().unwrap().failure {
                return Err(error(format!("player {}: {failure}", player + 1)));
            }
        }
        Ok(())
    }
    pub(crate) fn begin_tick(&self) -> Result<(), crate::Error> {
        self.check()?;
        for seat in &self.seats {
            seat.lock().unwrap().calls = 4096;
        }
        Ok(())
    }
    pub(crate) fn end_tick(&self) -> Result<(), crate::Error> {
        // No lifecycle report from a failed frame reaches the consumer.
        self.check()?;
        for seat in &self.seats {
            let mut seat = seat.lock().unwrap();
            if self.priming {
                if seat.pending.contains(&script::Event::MatchAborted) {
                    return Err(error("match aborted during startup"));
                }
                let mut ready = seat.ready;
                seat.pending.retain(|event| {
                    if *event == script::Event::Ready {
                        ready = true;
                        false
                    } else {
                        true
                    }
                });
                seat.ready = ready;
                continue;
            }
            for event in std::mem::take(&mut seat.pending) {
                match event {
                    script::Event::Ready => seat.ready = true,
                    script::Event::RoundStarted => self.events.round_started(),
                    script::Event::RoundOutcome { winner } => self.events.round_outcome(match winner {
                        Some(1) => Outcome::P0Win,
                        Some(2) => Outcome::P1Win,
                        None => Outcome::Draw,
                        _ => unreachable!("controller validated the outcome"),
                    }),
                    script::Event::MatchEnded => self.events.match_ended(),
                    script::Event::MatchAborted => self.events.match_aborted(),
                }
            }
        }
        Ok(())
    }
    fn ready(&self) -> bool {
        self.seats.iter().all(|seat| seat.lock().unwrap().ready)
    }
    pub(crate) fn snapshot(&self) -> Result<Snapshot, crate::Error> {
        self.check()?;
        Ok(Snapshot {
            seats: self.seats.each_ref().map(|seat| {
                let seat = seat.lock().unwrap();
                (seat.program.snapshot(), seat.ready)
            }),
        })
    }
    pub(crate) fn validate_snapshot(&self, snapshot: &Snapshot) -> Result<(), crate::Error> {
        for (seat, (state, _)) in self.seats.iter().zip(&snapshot.seats) {
            seat.lock().unwrap().program.validate_snapshot(state).map_err(error)?;
        }
        Ok(())
    }
    pub(crate) fn restore(&self, snapshot: &Snapshot) -> Result<(), crate::Error> {
        self.validate_snapshot(snapshot)?;
        for (seat, (state, ready)) in self.seats.iter().zip(&snapshot.seats) {
            let mut seat = seat.lock().unwrap();
            seat.program.restore(state).map_err(error)?;
            seat.ready = *ready;
            seat.pending.clear();
            seat.failure = None;
        }
        Ok(())
    }
}

pub struct Booted {
    pub link: crate::Link,
    pub telemetry: tango_match::telemetry::TelemetryHandle,
    pub prime_ticks: u32,
}

/// Boot with package-defined behavior, without a native game registration.
/// Programs must be opened against these exact effective ROMs and contexts.
pub fn boot(
    roms: [Vec<u8>; 2],
    saves: [Vec<u8>; 2],
    programs: [Box<dyn script::Program>; 2],
    rtc: std::time::SystemTime,
    render: bool,
    cancel: &AtomicBool,
) -> Result<Booted, crate::Error> {
    let rom_lengths = roms.each_ref().map(Vec::len);
    let mut pair = crate::backend::bare_pair(roms, saves, rtc, render)?;
    let events = EventSink::new();
    let gamemodes = Pair::install(&mut pair, programs, rom_lengths, events.clone())?;
    let mut prime_ticks = 0;
    while !gamemodes.ready() {
        if cancel.load(Ordering::Relaxed) {
            return Err(crate::Error::Cancelled);
        }
        if prime_ticks == 3600 {
            return Err(crate::Error::PrimeTimeout(3600));
        }
        gamemodes.begin_tick()?;
        pair.try_tick(&[0, 0])?;
        gamemodes.end_tick()?;
        prime_ticks += 1;
    }
    finish(pair, gamemodes, events, prime_ticks, true)
}

/// Only for replay workers immediately restoring a primed capture.
fn unprimed(
    roms: [Vec<u8>; 2],
    saves: [Vec<u8>; 2],
    programs: [Box<dyn script::Program>; 2],
    rtc: std::time::SystemTime,
) -> Result<Booted, crate::Error> {
    let lengths = roms.each_ref().map(Vec::len);
    let mut pair = crate::backend::bare_pair(roms, saves, rtc, true)?;
    let events = EventSink::new();
    let gamemodes = Pair::install(&mut pair, programs, lengths, events.clone())?;
    finish(pair, gamemodes, events, 0, false)
}

fn finish(
    mut pair: mgba_rollback::Link,
    mut gamemodes: Pair,
    events: EventSink,
    prime_ticks: u32,
    stamp: bool,
) -> Result<Booted, crate::Error> {
    gamemodes.priming = false;
    for player in 0..2 {
        let core = pair.core_mut(player);
        core.set_audio_buffer_size(16384);
        core.audio_buffer().clear();
    }
    // Lifecycle remains useful without a native observation poller. Packages
    // install their independent named-value samplers through Link::set_records.
    let (mut telemetry, handle) = Telemetry::new(
        [
            Box::new(|_: &mut mgba::core::Core| None),
            Box::new(|_: &mut mgba::core::Core| None),
        ],
        events,
    );
    gamemodes.end_tick()?;
    // Stamp priming lifecycle at the initial capture so restoring tick zero
    // retains it without relying on callbacks that already ran before boot.
    if stamp {
        telemetry.observe(None, None, 0);
    }
    let mut link = crate::Link::new(pair, Some(telemetry));
    link.gamemodes = Some(gamemodes);
    Ok(Booted {
        link,
        telemetry: handle,
        prime_ticks,
    })
}
