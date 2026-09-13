use std::sync::Arc;

use tango_match::Backend;
use tango_net_protocol::control::Settings;

/// A host-supplied backend, retained for the session and its workers.
/// Static implementations and dynamically prepared implementations share the
/// same contract; package identity comes from `Backend::gamemodes`.
#[derive(Clone)]
pub enum SessionBackend {
    Static(&'static (dyn Backend + Send + Sync)),
    Owned(Arc<dyn Backend + Send + Sync>),
}

impl std::ops::Deref for SessionBackend {
    type Target = dyn Backend + Send + Sync;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Static(backend) => *backend,
            Self::Owned(backend) => backend.as_ref(),
        }
    }
}

impl SessionBackend {
    pub(crate) fn validate_match(
        &self,
        settings: [&Settings; 2],
        local_player: usize,
        match_type: u8,
        disable_bgm: bool,
    ) -> Result<(), tango_match::Error> {
        let Some(configurations) = self.gamemodes() else {
            if settings.iter().any(|settings| settings.gamemode.is_some()) {
                return Err(tango_match::Error::Unsupported("missing resolved package backend"));
            }
            return Ok(());
        };
        validate_package(configurations, settings, local_player, match_type, disable_bgm)
    }

    pub fn is_package(&self) -> bool {
        self.gamemodes().is_some()
    }

    pub(crate) fn validate_replay(
        &self,
        metadata: &tango_replay::Metadata,
        match_type: u8,
    ) -> Result<(), crate::Error> {
        let recorded = metadata.gamemodes()?;
        match (self.gamemodes(), recorded) {
            (None, None) => Ok(()),
            (Some(configurations), Some(recorded))
                if configurations == &recorded
                    && match_type == 0
                    && metadata.match_type == 0
                    && metadata.match_subtype == 0 =>
            {
                Ok(())
            }
            _ => Err(tango_match::Error::Unsupported("replay requires its exact recorded package backend").into()),
        }
    }
}

fn validate_package(
    configurations: &[tango_match::gamemode::Configuration; 2],
    settings: [&Settings; 2],
    local_player: usize,
    match_type: u8,
    disable_bgm: bool,
) -> Result<(), tango_match::Error> {
    if local_player >= 2 || match_type != 0 || disable_bgm {
        return Err(tango_match::Error::Unsupported(
            "package behavior must come from its frozen configuration",
        ));
    }
    for (perspective, settings) in settings.into_iter().enumerate() {
        let expected = &configurations[local_player ^ perspective];
        expected.validate().map_err(tango_match::Error::Backend)?;
        if settings.match_type != 0 || settings.gamemode.as_ref() != Some(expected) {
            return Err(tango_match::Error::Unsupported(
                "package backend differs from the committed gamemodes",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tango_match::gamemode::{
        identity::{Identity, Package, Runtime},
        Configuration, OptionValue,
    };

    fn configuration(player: u8) -> Configuration {
        Configuration::new(
            Identity {
                engine: Runtime {
                    name: "test".into(),
                    revision: 1,
                },
                script: Runtime {
                    name: "luau".into(),
                    revision: 2,
                },
                package: Package {
                    name: "game".into(),
                    version: "1.0.0".into(),
                    digest: [1; 32],
                },
                export: "single".into(),
                dependencies: Vec::new(),
                rom: [player; 32],
                environment: [player; 32],
            },
            false,
            Default::default(),
        )
        .unwrap()
    }

    struct TestBackend(Option<[Configuration; 2]>);

    impl Backend for TestBackend {
        fn gamemodes(&self) -> Option<&[Configuration; 2]> {
            self.0.as_ref()
        }
        fn sim_version(&self) -> u32 {
            1
        }
        fn screen_layout(&self, _: tango_match::SessionMode) -> tango_match::ScreenLayout {
            tango_match::ScreenLayout::single(16, 16)
        }
        fn keys_mask(&self) -> u32 {
            1
        }
        fn tps(&self) -> num_rational::Ratio<u32> {
            num_rational::Ratio::new(60, 1)
        }
        fn start(&self, _: tango_match::StartConfig) -> Result<tango_match::Match, tango_match::Error> {
            panic!("validation must not start emulation")
        }
    }

    #[test]
    fn backend_ownership_does_not_determine_package_identity() {
        static NATIVE: TestBackend = TestBackend(None);
        let settings = Settings::default();
        let package_settings = Settings {
            gamemode: Some(configuration(1)),
            ..Default::default()
        };
        for backend in [
            SessionBackend::Static(&NATIVE),
            SessionBackend::Owned(Arc::new(TestBackend(None))),
        ] {
            assert!(!backend.is_package());
            backend.validate_match([&settings, &settings], 0, 0, false).unwrap();
            assert!(backend
                .validate_match([&settings, &package_settings], 0, 0, false)
                .is_err());
        }
        let backend = SessionBackend::Owned(Arc::new(TestBackend(Some([configuration(1), configuration(1)]))));
        assert!(backend.is_package());
        backend
            .validate_match([&package_settings, &package_settings], 0, 0, false)
            .unwrap();
        assert!(backend.validate_match([&settings, &settings], 0, 0, false).is_err());
    }

    #[test]
    fn replays_require_the_exact_package_backend_without_a_native_registration() {
        let configurations = [configuration(1), configuration(2)];
        let sides = configurations.each_ref().map(|config| {
            Some(tango_replay::metadata::Side {
                gamemode: config.encode().unwrap(),
                ..Default::default()
            })
        });
        let mut metadata = tango_replay::Metadata {
            p1_side: sides[0].clone(),
            p2_side: sides[1].clone(),
            ..Default::default()
        };
        let backend = SessionBackend::Owned(Arc::new(TestBackend(Some(configurations.clone()))));
        backend.validate_replay(&metadata, 0).unwrap();
        assert!(backend.validate_replay(&metadata, 1).is_err());
        let native = SessionBackend::Owned(Arc::new(TestBackend(None)));
        assert!(native.validate_replay(&metadata, 0).is_err());
        let swapped = SessionBackend::Owned(Arc::new(TestBackend(Some([
            configurations[1].clone(),
            configurations[0].clone(),
        ]))));
        assert!(swapped.validate_replay(&metadata, 0).is_err());
        metadata.match_subtype = 1;
        assert!(backend.validate_replay(&metadata, 0).is_err());
        metadata.match_subtype = 0;
        metadata.p2_side = None;
        assert!(backend.validate_replay(&metadata, 0).is_err());
        assert!(native.validate_replay(&metadata, 0).is_err());
        let native_metadata = tango_replay::Metadata::default();
        assert!(backend.validate_replay(&native_metadata, 0).is_err());
        native.validate_replay(&native_metadata, 0).unwrap();
    }

    #[test]
    fn owned_backends_must_match_the_committed_seats_in_absolute_order() {
        let configurations = [configuration(1), configuration(2)];
        let settings = configurations.each_ref().map(|configuration| Settings {
            gamemode: Some(configuration.clone()),
            ..Default::default()
        });
        for local_player in [0, 1] {
            let [local, remote] = [&settings[local_player], &settings[1 - local_player]];
            validate_package(&configurations, [local, remote], local_player, 0, false).unwrap();
            assert!(validate_package(&configurations, [remote, local], local_player, 0, false).is_err());
            for (match_type, disable_bgm) in [(1, false), (0, true)] {
                assert!(
                    validate_package(&configurations, [local, remote], local_player, match_type, disable_bgm).is_err()
                );
            }
            let mut changed = remote.clone();
            changed
                .gamemode
                .as_mut()
                .unwrap()
                .options
                .insert("changed".into(), OptionValue::Boolean(true));
            assert!(validate_package(&configurations, [local, &changed], local_player, 0, false).is_err());
            changed.gamemode = None;
            assert!(validate_package(&configurations, [local, &changed], local_player, 0, false).is_err());
        }
        assert!(validate_package(&configurations, [&settings[0], &settings[1]], 2, 0, false).is_err());
    }
}
