//! Exercise replay preparation without emulating or needing a real ROM.

use super::*;
use crate::{game, rom, storage::StdStorage};
use std::path::{Path, PathBuf};

struct Fixture {
    patches: PathBuf,
    roms: rom::Scanner,
    games: [rom::GameRef; 2],
    metadata: tango_replay::Metadata,
}

impl Fixture {
    fn new() -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let patches = std::env::temp_dir().join(format!("tango-resolve-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(&patches).unwrap();
        let games = [
            game::find_by_rom_info(b"BR5E", 0).unwrap(),
            game::find_by_rom_info(b"BR6E", 0).unwrap(),
        ];
        let roms = rom::Scanner::new();
        roms.rescan(|| Some([(games[0], b"rom".to_vec()), (games[1], b"rom".to_vec())].into()));
        let side = |game: rom::GameRef| {
            Some(tango_replay::metadata::Side {
                game_info: Some(tango_replay::metadata::GameInfo {
                    rom_family: game.family.id.into(),
                    rom_variant: game.variant as u32,
                    sim_version: game.pvp.sim_version(),
                    patch: None,
                }),
                ..Default::default()
            })
        };
        let metadata = tango_replay::Metadata {
            p1_side: side(games[0]),
            p2_side: side(games[1]),
            ..Default::default()
        };
        Self {
            patches,
            roms,
            games,
            metadata,
        }
    }

    fn patch(&mut self, player: u8, version: &str) {
        let side = if player == 0 {
            &mut self.metadata.p1_side
        } else {
            &mut self.metadata.p2_side
        };
        side.as_mut().unwrap().game_info.as_mut().unwrap().patch = Some(tango_replay::metadata::game_info::Patch {
            name: "test".into(),
            version: version.into(),
        });
    }

    fn resolve(&self) -> Result<ResolvedRoms, ResolveError> {
        resolve_roms(&StdStorage, &self.roms, &self.patches, &self.metadata)
    }

    fn install(&self, version: &str, bps: &[u8]) {
        let manifest = tango_patch::Manifest::parse(&format!(
            r#"
format = 2
name = "test"
version = "{version}"
title = "Resolver fixture"
authors = []
netplay = "group:test"
"#
        ))
        .unwrap();
        let mut builder = tango_patch::bundle::Builder::new(manifest);
        for game in self.games {
            builder.add_rom(tango_patch::RomTarget::new(*game.rom_code, game.revision), bps.to_vec());
        }
        builder.write_file(&self.patches).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.patches);
    }
}

// BPS TargetRead fixtures replacing the three bytes "rom" with "one"/"two".
const ONE: &[u8] = &[
    0x42, 0x50, 0x53, 0x31, 0x83, 0x83, 0x80, 0x89, 0x6f, 0x6e, 0x65, 0xa1, 0x0f, 0x52, 0x79, 0xf1, 0x86, 0x6c, 0x7a,
    0x3a, 0xd8, 0xac, 0x51,
];
const TWO: &[u8] = &[
    0x42, 0x50, 0x53, 0x31, 0x83, 0x83, 0x80, 0x89, 0x74, 0x77, 0x6f, 0xa1, 0x0f, 0x52, 0x79, 0x66, 0x8a, 0xca, 0x11,
    0xcf, 0x38, 0x5e, 0x99,
];

#[test]
fn resolves_in_absolute_seat_order_without_mutating_the_library() {
    let fixture = Fixture::new();
    let mut resolved = fixture.resolve().unwrap();
    assert_eq!(resolved.games, fixture.games);
    resolved.roms[0][0] = b'X';
    assert_eq!(fixture.roms.read()[&fixture.games[0]], b"rom");
    assert_eq!(resolved.roms[1], b"rom");
}

#[test]
fn applies_each_seats_exact_recorded_patch_version() {
    let mut fixture = Fixture::new();
    fixture.install("1.0.0", ONE);
    fixture.install("2.0.0", TWO);
    fixture.patch(0, "1.0.0");
    fixture.patch(1, "2.0.0");
    let resolved = fixture.resolve().unwrap();
    assert_eq!(resolved.roms, [b"one".to_vec(), b"two".to_vec()]);
    assert_eq!(fixture.roms.read()[&fixture.games[0]], b"rom");
}

#[test]
fn missing_recorded_patch_does_not_fall_back_to_newer_or_clean_rom() {
    let mut fixture = Fixture::new();
    fixture.install("2.0.0", TWO);
    fixture.patch(0, "1.0.0");
    assert!(matches!(
        fixture.resolve(),
        Err(ResolveError::Rom {
            player: 1,
            source: rom::LoadError::Patch { .. }
        })
    ));
}

#[test]
fn reports_the_seat_with_a_missing_rom() {
    let fixture = Fixture::new();
    fixture
        .roms
        .rescan(|| Some([(fixture.games[0], b"rom".to_vec())].into()));
    assert!(matches!(
        fixture.resolve(),
        Err(ResolveError::Rom {
            player: 2,
            source: rom::LoadError::MissingRom { .. }
        })
    ));
}

#[test]
fn rejects_incompatible_simulation_and_malformed_patch_version() {
    let mut fixture = Fixture::new();
    fixture
        .metadata
        .p1_side
        .as_mut()
        .unwrap()
        .game_info
        .as_mut()
        .unwrap()
        .sim_version += 1;
    assert!(matches!(
        fixture.resolve(),
        Err(ResolveError::Game {
            player: 1,
            source: game::ReplaySideError::SimVersionMismatch { .. }
        })
    ));
    fixture
        .metadata
        .p1_side
        .as_mut()
        .unwrap()
        .game_info
        .as_mut()
        .unwrap()
        .sim_version = fixture.games[0].pvp.sim_version();
    fixture.patch(1, "not-a-version");
    assert!(matches!(
        fixture.resolve(),
        Err(ResolveError::PatchVersion { player: 2, .. })
    ));
}

#[test]
fn rejects_missing_game_metadata() {
    assert!(matches!(
        resolve_roms(
            &StdStorage,
            &rom::Scanner::new(),
            Path::new("unused"),
            &Default::default()
        ),
        Err(ResolveError::MissingGameInfo { player: 1 })
    ));
}
