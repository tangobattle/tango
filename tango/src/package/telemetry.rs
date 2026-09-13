//! Bind installed telemetry exports to immutable ROM inputs at session boot.
use std::sync::Arc;

use tango_match::telemetry::stream;
use tango_script::{ExportKind, Inputs, Profile, TelemetrySampler};

pub struct Factory {
    bound: std::sync::Mutex<std::collections::BTreeMap<[u8; 32], Profile>>,
    profiles: Vec<Profile>,
}

/// Freeze the catalog for this session. Legacy patches still carry native
/// overrides, so their telemetry is not automatically assigned a base package.
pub fn factory(catalog: &tango_library::package::Catalog, legacy_patch: bool) -> Option<Arc<Factory>> {
    if legacy_patch {
        return None;
    }
    let profiles: Vec<_> = catalog
        .latest_exports(ExportKind::Telemetry)
        .into_iter()
        .map(|export| export.profile.clone())
        .collect();
    (!profiles.is_empty()).then(|| {
        Arc::new(Factory {
            profiles,
            bound: Default::default(),
        })
    })
}

impl stream::Factory for Factory {
    fn open(&self, rom: &[u8], _player: u16) -> Result<Option<Box<dyn stream::Sampler>>, stream::Error> {
        let mut selected = None;
        for profile in &self.profiles {
            if profile.supports_export(ExportKind::Telemetry, rom)? {
                if selected.is_some() {
                    return Err("multiple package telemetry exports recognize this ROM".into());
                }
                selected = Some(profile.clone());
            }
        }
        let Some(profile) = selected else {
            return Ok(None);
        };
        let inputs = Inputs::new([("rom".to_owned(), rom.to_vec())].into())?;
        let profile = profile.with_inputs(inputs);
        let sampler = TelemetrySampler::new(profile.clone())?;
        self.bound.lock().unwrap().insert(profile.digest(), profile);
        Ok(Some(Box::new(sampler)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stream::Factory as _;

    fn profile() -> Profile {
        let package = tango_script::Package::load(
            [
                (
                    "package.toml".into(),
                    b"api=1\nname='observer'\nversion='1.0.0'\n[[telemetry]]\nname='main'\npath='./telemetry'\n"
                        .to_vec(),
                ),
                (
                    "init.luau".into(),
                    b"--!strict\nerror('library entry must not run during telemetry')".to_vec(),
                ),
                (
                    "telemetry.luau".into(),
                    br#"--!strict
return {
    detect_rom = function(rom: buffer): boolean
        return buffer.len(rom) == 1 and buffer.readu8(rom, 0) == 7
    end,
    initial_state = function(): buffer return tango.read_input("rom", 0, 1) end,
    view = function(history: TelemetryHistory, context: TelemetryViewContext, state: ViewState): Node
        return {kind = "text", text = tostring(history.value({player = context.player, name = "counter", tick = context.to_tick})) .. (state.caption or "")}
    end,
    update_view = function(state: ViewState, action: Action): ViewState state.caption = action.id; return state end,
    poll = function(state: buffer, _context: TelemetryContext, memory: Memory): TelemetryFrame
        local value = buffer.readu8(state, 0) + buffer.readu8(memory.read("main", 0, 1), 0)
        buffer.writeu8(state, 0, value)
        return {values = {counter = value}, events = {}}
    end,
}
"#
                    .to_vec(),
                ),
            ]
            .into(),
        )
        .unwrap();
        Profile::resolve(
            &[package],
            &tango_script::PackageRef {
                name: "observer".into(),
                version: "1.0.0".parse().unwrap(),
            },
        )
        .unwrap()
    }

    struct Memory;
    impl stream::Memory for Memory {
        fn read(&mut self, _: &str, _: u32, output: &mut [u8]) -> Result<(), stream::Error> {
            output.fill(5);
            Ok(())
        }
    }

    #[test]
    fn selected_telemetry_binds_rom_inputs_and_restores_lua_state() {
        let factory = Factory {
            bound: Default::default(),
            profiles: vec![profile()],
        };
        assert!(factory.open(&[8], 1).unwrap().is_none());
        let (mut collector, store) = stream::Collector::new(&factory, [&[7], &[7]]);
        let initial = collector.snapshot();
        collector.poll(0, &mut Memory, 1, 1);
        collector.poll(1, &mut Memory, 1, 1);
        let expected = store.lock().unwrap().take_through(1).records;
        for record in &expected {
            assert_eq!(
                record.frame.as_ref().unwrap().values["counter"],
                stream::Value::Number(12.0)
            );
        }
        assert_eq!(expected.len(), 2);
        collector.poll(0, &mut Memory, 2, 1);
        collector.restore(&initial, 0).unwrap();
        collector.poll(0, &mut Memory, 1, 1);
        collector.poll(1, &mut Memory, 1, 1);
        let repeated = store.lock().unwrap().take_through(1);
        assert_eq!(repeated.rewind_to, Some(0));
        assert_eq!(repeated.records, expected);
        let mut timeline = stream::Timeline::new(store.lock().unwrap().sources().clone());
        timeline.apply(repeated);
        let panel = factory.panel(timeline, 1, 60.0, "en-US").unwrap().unwrap();
        let before = panel.controller.lock().unwrap().node.clone();
        panel.update(&tango_script::Action::activate("clicked", ""));
        assert_ne!(before, panel.controller.lock().unwrap().node);
        let _ = panel.view("ja-JP");
        assert_eq!(panel.controller.lock().unwrap().locale, "ja-JP");
    }

    #[test]
    fn ambiguous_detection_is_reported_without_selecting_an_arbitrary_export() {
        let profile = profile();
        let factory = Factory {
            bound: Default::default(),
            profiles: vec![profile.clone(), profile],
        };
        assert!(factory.open(&[7], 1).err().unwrap().to_string().contains("multiple"));
        assert!(super::factory(&tango_library::package::Catalog::default(), true).is_none());
    }
}

/// A package-owned view over the frozen, confirmed history of one match.
pub struct Panel {
    history: stream::Timeline,
    context: tango_script::TelemetryViewContext,
    controller: std::sync::Mutex<Controller>,
    renderer: tango_script_iced::Renderer,
}
struct Controller {
    profile: Profile,
    locale: String,
    state: std::collections::BTreeMap<String, String>,
    node: tango_script::Node,
}
impl Factory {
    pub fn panel(
        &self,
        history: stream::Timeline,
        player: u16,
        ticks_per_second: f64,
        locale: &str,
    ) -> tango_script::Result<Option<Panel>> {
        if !(1..=2).contains(&player) {
            return Err(tango_script::Error::Invalid("invalid telemetry player".into()));
        }
        let Some(source) = &history.sources[usize::from(player - 1)] else {
            return Ok(None);
        };
        let Some(profile) = self.bound.lock().unwrap().get(&source.digest).cloned() else {
            return Ok(None);
        };
        let profile = profile.with_locale(locale)?;
        let (from_tick, to_tick) = history.extent().unwrap_or((0, 0));
        let context = tango_script::TelemetryViewContext {
            player,
            from_tick,
            to_tick,
            ticks_per_second,
            incomplete: false,
        };
        let state = Default::default();
        let Some(node) = profile.view_telemetry(&history, context, &state)? else {
            return Ok(None);
        };
        Ok(Some(Panel {
            history,
            context,
            controller: std::sync::Mutex::new(Controller {
                profile,
                locale: locale.into(),
                state,
                node,
            }),
            renderer: Default::default(),
        }))
    }
}
impl Panel {
    pub fn view(&self, locale: &str) -> iced::Element<'static, tango_script_iced::Message> {
        let mut controller = self.controller.lock().unwrap();
        if controller.locale != locale {
            controller.locale = locale.into();
            let render = || -> tango_script::Result<_> {
                let profile = controller.profile.clone().with_locale(locale)?;
                let node = profile.view_telemetry(&self.history, self.context, &controller.state)?;
                Ok((profile, node))
            };
            match render() {
                Ok((profile, Some(node))) => {
                    controller.profile = profile;
                    controller.node = node;
                }
                Ok((_, None)) => {}
                Err(error) => log::warn!("package telemetry view: {error}"),
            }
        }
        self.renderer.view(&controller.node)
    }
    pub fn update(&self, action: &tango_script::Action) {
        let mut controller = self.controller.lock().unwrap();
        let update = || -> tango_script::Result<_> {
            let state = controller.profile.update_telemetry_view(&controller.state, action)?;
            if state == controller.state {
                return Ok((state, None));
            }
            let node = controller.profile.view_telemetry(&self.history, self.context, &state)?;
            Ok((state, node))
        };
        match update() {
            Ok((state, Some(node))) => {
                controller.state = state;
                controller.node = node;
            }
            Ok((_, None)) => {}
            Err(error) => log::warn!("package telemetry action: {error}"),
        }
    }
}
