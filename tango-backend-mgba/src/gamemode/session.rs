//! The ordinary session/replay backend backed entirely by package factories.
use std::sync::{atomic::AtomicBool, Arc};

use tango_match::gamemode::{Configuration, Factory, Program};

#[derive(Clone)]
pub struct Backend {
    seats: [Arc<dyn Factory>; 2],
    configurations: [Configuration; 2],
}

impl Backend {
    /// Seats are in absolute player order and must already have been resolved
    /// against their advertised/recorded configurations by the host.
    pub fn new(seats: [Arc<dyn Factory>; 2]) -> Result<Self, tango_match::Error> {
        let configurations = [
            seats[0].configuration().map_err(super::error)?,
            seats[1].configuration().map_err(super::error)?,
        ];
        for configuration in &configurations {
            configuration.validate().map_err(super::error)?;
            if configuration.identity.engine != super::runtime() {
                return Err(super::error("gamemode requires a different engine ABI").into());
            }
        }
        Ok(Self { seats, configurations })
    }

    pub fn configurations(&self) -> &[Configuration; 2] {
        &self.configurations
    }

    fn roms(&self) -> [Vec<u8>; 2] {
        self.seats.each_ref().map(|seat| seat.rom().to_vec())
    }

    fn programs(&self, seed: [u8; 16]) -> Result<[Box<dyn Program>; 2], crate::Error> {
        Ok([
            self.seats[0].open(1, seed).map_err(super::error)?,
            self.seats[1].open(2, seed).map_err(super::error)?,
        ])
    }

    fn validate(
        &self,
        local_player: usize,
        roms: [&[u8]; 2],
        match_type: u8,
        disable_bgm: bool,
    ) -> Result<(), tango_match::Error> {
        if local_player >= 2 || roms.iter().zip(&self.seats).any(|(rom, seat)| *rom != seat.rom()) {
            return Err(super::error("session inputs do not match the prepared gamemode seats").into());
        }
        // These are legacy StartConfig/ReplayConfig knobs. Scripted behavior
        // comes exclusively from the options frozen into each factory.
        if match_type != 0 || disable_bgm {
            return Err(super::error("configure package gamemode options before creating a session").into());
        }
        Ok(())
    }
}

impl tango_match::Backend for Backend {
    fn gamemodes(&self) -> Option<&[Configuration; 2]> {
        Some(&self.configurations)
    }

    /// Only the hardware ABI. Package sessions additionally require both full
    /// configurations; this number cannot stand in for their content identity.
    fn sim_version(&self) -> u32 {
        super::runtime().revision
    }
    fn screen_layout(&self, _: tango_match::SessionMode) -> tango_match::ScreenLayout {
        crate::link::screen_layout()
    }
    fn keys_mask(&self) -> u32 {
        crate::link::JOYFLAGS_MASK
    }
    fn tps(&self) -> num_rational::Ratio<u32> {
        crate::link::TPS
    }

    fn start_solo(&self, config: tango_match::SoloConfig) -> Result<tango_match::Solo, tango_match::Error> {
        if !self.seats.iter().any(|seat| seat.rom() == config.rom) {
            return Err(super::error("solo ROM does not match the prepared gamemode").into());
        }
        Ok(tango_match::Solo::new(
            crate::solo::SoloConsole::new(config.rom, config.save, config.rtc).map_err(tango_match::Error::from)?,
            config.audio,
        ))
    }

    fn start(&self, config: tango_match::StartConfig) -> Result<tango_match::Match, tango_match::Error> {
        self.validate(config.local_player, config.roms, config.match_type, config.disable_bgm)?;
        let cancel = AtomicBool::new(false);
        let mut booted = super::boot(
            self.roms(),
            config.saves.map(|save| save.unwrap_or_default().to_vec()),
            self.programs(config.rng_seed)?,
            config.rtc,
            true,
            config.cancel.unwrap_or(&cancel),
        )?;
        let records = config.record_factory.map(|factory| {
            let (collector, handle) = tango_match::telemetry::stream::Collector::new(factory, config.roms);
            booted.link.set_records(collector);
            handle
        });
        let mut session =
            tango_match::Match::new(booted.link, config.local_player, config.present_delay, config.audio)?;
        session.set_telemetry(booted.telemetry);
        if let Some(records) = records {
            session.set_records(records);
        }
        Ok(session)
    }

    fn open_replay(&self, config: tango_match::ReplayConfig) -> Result<tango_match::ReplaySet, tango_match::Error> {
        self.validate(
            config.local_player,
            [&config.roms[0], &config.roms[1]],
            config.match_type,
            config.disable_bgm,
        )?;
        let boot = Boot {
            backend: self.clone(),
            saves: config.saves.clone(),
            seed: config.rng_seed,
            rtc: config.rtc,
            record_factory: config.record_factory.clone(),
        };
        Ok(tango_match::ReplaySet::new(&config, boot))
    }
}

struct Boot {
    backend: Backend,
    saves: [Vec<u8>; 2],
    seed: [u8; 16],
    rtc: std::time::SystemTime,
    record_factory: Option<Arc<dyn tango_match::telemetry::stream::Factory>>,
}

impl Boot {
    fn wrap(&self, mut booted: super::Booted, observe: bool) -> tango_match::BootedReplay {
        let records = observe
            .then_some(self.record_factory.as_ref())
            .flatten()
            .map(|factory| {
                let roms = self.backend.seats.each_ref().map(|seat| seat.rom());
                let (collector, handle) = tango_match::telemetry::stream::Collector::new(factory.as_ref(), roms);
                booted.link.set_records(collector);
                handle
            });
        tango_match::BootedReplay {
            link: Box::new(booted.link),
            telemetry: observe.then_some(booted.telemetry),
            records,
        }
    }
}

impl tango_match::ReplayBoot for Boot {
    fn boot(&self, observe: bool, cancel: &AtomicBool) -> Result<tango_match::BootedReplay, tango_match::Error> {
        Ok(self.wrap(
            super::boot(
                self.backend.roms(),
                self.saves.clone(),
                self.backend.programs(self.seed)?,
                self.rtc,
                true,
                cancel,
            )?,
            observe,
        ))
    }

    fn boot_unprimed(&self, observe: bool) -> Result<tango_match::BootedReplay, tango_match::Error> {
        Ok(self.wrap(
            super::unprimed(
                self.backend.roms(),
                self.saves.clone(),
                self.backend.programs(self.seed)?,
                self.rtc,
            )?,
            observe,
        ))
    }
}
