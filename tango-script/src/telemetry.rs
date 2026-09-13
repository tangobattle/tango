//! Bounded telemetry over immutable host inputs. State is explicit so a host
//! can rewind it with its emulator instead of retaining speculative Lua globals.
pub use tango_match::telemetry::stream::{
    Context as TelemetryContext, Event as TelemetryEvent, Frame as TelemetryFrame, Value as TelemetryValue,
};

/// An owned sampler whose cheap clones are complete rollback checkpoints.
/// Immutable package/code/input identity travels with its private state.
#[derive(Clone)]
pub struct TelemetrySampler {
    profile: std::sync::Arc<crate::Profile>,
    source: std::sync::Arc<tango_match::telemetry::stream::Source>,
    state: std::sync::Arc<[u8]>,
}

impl TelemetrySampler {
    pub fn new(profile: crate::Profile) -> crate::Result<Self> {
        let state = profile.initial_telemetry_state()?.into();
        let source = std::sync::Arc::new(tango_match::telemetry::stream::Source {
            digest: profile.digest(),
            name: profile.display_name().to_owned(),
        });
        Ok(Self {
            profile: std::sync::Arc::new(profile),
            source,
            state,
        })
    }
}

impl tango_match::telemetry::stream::Sampler for TelemetrySampler {
    fn source(&self) -> &tango_match::telemetry::stream::Source {
        &self.source
    }

    fn sample(
        &mut self,
        memory: &mut dyn tango_match::telemetry::stream::Memory,
        context: TelemetryContext,
    ) -> Result<TelemetryFrame, tango_match::telemetry::stream::Error> {
        let sample = self
            .profile
            .poll_telemetry_with_memory(&self.state, context, |space, address, output| {
                memory
                    .read(space, address, output)
                    .map_err(|error| crate::invalid(error.to_string()))
            })?;
        self.state = sample.state.into();
        Ok(sample.frame)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TelemetrySample {
    pub state: Vec<u8>,
    pub frame: TelemetryFrame,
}

impl crate::Profile {
    /// Let the package define its private state layout. Stateless pollers may
    /// omit the callback and start with an empty buffer.
    pub fn initial_telemetry_state(&self) -> crate::Result<Vec<u8>> {
        crate::runtime::Runtime::for_export(self, crate::ExportKind::Telemetry)?.initial_telemetry_state()
    }

    /// Poll the selected telemetry export without loading the editor or match
    /// mode. The caller retains the previous state if the operation fails.
    pub fn poll_telemetry(&self, state: &[u8], context: TelemetryContext) -> crate::Result<TelemetrySample> {
        self.poll_telemetry_with_memory(state, context, |_, _, _| {
            Err(crate::invalid("the host did not supply a memory reader"))
        })
    }

    /// Read the current emulator state without copying entire memory banks.
    /// The host defines address-space names and must use side-effect-free reads.
    /// The borrowed reader exists only during this synchronous call; scripts
    /// cannot retain it across ticks or use it to write memory.
    pub fn poll_telemetry_with_memory<F>(
        &self,
        state: &[u8],
        context: TelemetryContext,
        read: F,
    ) -> crate::Result<TelemetrySample>
    where
        F: FnMut(&str, u32, &mut [u8]) -> crate::Result<()>,
    {
        crate::runtime::Runtime::for_export(self, crate::ExportKind::Telemetry)?.poll_telemetry(state, context, read)
    }
}

/// A presentation window over confirmed telemetry. Positions are 1-based.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct TelemetryViewContext {
    pub player: u16,
    pub from_tick: u32,
    pub to_tick: u32,
    pub ticks_per_second: f64,
    pub incomplete: bool,
}
impl crate::Profile {
    pub fn view_telemetry(
        &self,
        history: &tango_match::telemetry::stream::Timeline,
        mut context: TelemetryViewContext,
        state: &std::collections::BTreeMap<String, String>,
    ) -> crate::Result<Option<crate::Node>> {
        if !(1..=2).contains(&context.player)
            || context.from_tick > context.to_tick
            || !context.ticks_per_second.is_finite()
            || context.ticks_per_second <= 0.0
        {
            return Err(crate::invalid("invalid telemetry view context"));
        }
        context.incomplete = history.limited_at().is_some() || !history.errors().is_empty();
        crate::runtime::Runtime::for_export(self, crate::ExportKind::Telemetry)?.telemetry_view(history, context, state)
    }
    pub fn update_telemetry_view(
        &self,
        state: &std::collections::BTreeMap<String, String>,
        action: &crate::Action,
    ) -> crate::Result<std::collections::BTreeMap<String, String>> {
        crate::runtime::Runtime::for_export(self, crate::ExportKind::Telemetry)?.update_telemetry_view(state, action)
    }
}
