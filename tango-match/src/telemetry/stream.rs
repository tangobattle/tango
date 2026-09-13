//! Game-neutral telemetry records. Sampling is observational; its private state
//! rides in emulator checkpoints, while consumers receive confirmed records and
//! explicit revocations when an observed replay seeks backwards.
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

mod query;
pub use query::{Bucket, Range, ValueAt};
mod timeline;
pub use timeline::{Point, Series, Timeline};

pub type Error = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Context {
    pub tick: u32,
    pub round: u32,
    /// 1-based position in the match.
    pub player: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum Value {
    Boolean(bool),
    Number(f64),
    String(String),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub name: String,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub values: BTreeMap<String, Value>,
    pub events: Vec<Event>,
}

/// Emulator adapters expose only side-effect-free reads. Spaces are named by
/// the backend; neither this interface nor the collector knows game addresses.
pub trait Memory {
    fn read(&mut self, space: &str, address: u32, output: &mut [u8]) -> Result<(), Error>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    /// Immutable identity of the code and inputs used by this sampler.
    pub digest: [u8; 32],
    pub name: String,
}

pub trait Sampler: super::PollerState {
    fn source(&self) -> &Source;
    fn sample(&mut self, memory: &mut dyn Memory, context: Context) -> Result<Frame, Error>;
}

pub trait Factory: Send + Sync {
    /// `None` means this ROM has no selected telemetry export.
    fn open(&self, rom: &[u8], player: u16) -> Result<Option<Box<dyn Sampler>>, Error>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub context: Context,
    pub frame: Result<Frame, String>,
}

#[derive(Default)]
pub struct Batch {
    /// Discard previously consumed records after this tick before appending.
    pub rewind_to: Option<u32>,
    pub records: Vec<Record>,
}

pub type Handle = Arc<Mutex<Store>>;

/// Pending observations, not a permanent history. Hosts drain the confirmed
/// prefix into their own analysis/presentation state. A stalled consumer cannot
/// make an untrusted sampler grow the queue without bound.
pub struct Store {
    sources: [Option<Source>; 2],
    records: VecDeque<Record>,
    bytes: usize,
    observed: u32,
    consumed: u32,
    rewind_to: Option<u32>,
}

const MAX_PENDING_BYTES: usize = 16 * 1024 * 1024;

impl Store {
    pub fn sources(&self) -> &[Option<Source>; 2] {
        &self.sources
    }

    pub fn take_through(&mut self, tick: u32) -> Batch {
        let tick = tick.min(self.observed);
        let mut records = Vec::new();
        while self.records.front().is_some_and(|record| record.context.tick <= tick) {
            let record = self.records.pop_front().unwrap();
            self.bytes -= weight(&record);
            records.push(record);
        }
        self.consumed = self.consumed.max(tick);
        Batch {
            rewind_to: self.rewind_to.take(),
            records,
        }
    }

    fn push(&mut self, record: Record) -> bool {
        let bytes = weight(&record);
        if bytes > MAX_PENDING_BYTES.saturating_sub(self.bytes) && record.frame.is_ok() {
            return false;
        }
        // At most one bounded failure per source can sit beyond the data cap.
        self.bytes += bytes;
        self.records.push_back(record);
        true
    }

    fn rewind(&mut self, tick: u32) {
        while self.records.back().is_some_and(|record| record.context.tick > tick) {
            self.bytes -= weight(&self.records.pop_back().unwrap());
        }
        self.observed = tick;
        if tick < self.consumed {
            self.rewind_to = Some(self.rewind_to.map_or(tick, |earlier| earlier.min(tick)));
            self.consumed = tick;
        }
    }
}

fn weight(record: &Record) -> usize {
    fn fields(fields: &BTreeMap<String, Value>) -> usize {
        fields.iter().fold(0usize, |total, (key, value)| {
            let string = if let Value::String(value) = value {
                value.len()
            } else {
                0
            };
            total
                .saturating_add(128)
                .saturating_add(key.len())
                .saturating_add(string)
        })
    }
    match &record.frame {
        Ok(frame) => frame
            .events
            .iter()
            .fold(128usize.saturating_add(fields(&frame.values)), |total, event| {
                total
                    .saturating_add(128)
                    .saturating_add(event.name.len())
                    .saturating_add(fields(&event.fields))
            }),
        Err(error) => 128 + error.len(),
    }
}

pub struct Snapshot {
    sources: [Option<[u8; 32]>; 2],
    // Captured pollers are Send; the mutex permits replay workers to share
    // an immutable emulator capture even when a poller's state is not Sync.
    states: Mutex<[Option<super::PollerSnapshot>; 2]>,
    disabled: [bool; 2],
}

pub struct Collector {
    samplers: [Option<Box<dyn Sampler>>; 2],
    disabled: [bool; 2],
    store: Handle,
}

impl Collector {
    pub fn new(factory: &dyn Factory, roms: [&[u8]; 2]) -> (Self, Handle) {
        let mut errors: [Option<String>; 2] = [None, None];
        let samplers: [Option<Box<dyn Sampler>>; 2] =
            std::array::from_fn(|player| match factory.open(roms[player], player as u16 + 1) {
                Ok(sampler) => sampler,
                Err(error) => {
                    errors[player] = Some(bounded_error(error));
                    None
                }
            });
        let mut store = Store {
            sources: std::array::from_fn(|player| samplers[player].as_ref().map(|sampler| sampler.source().clone())),
            records: VecDeque::new(),
            bytes: 0,
            observed: 0,
            consumed: 0,
            rewind_to: None,
        };
        for (player, error) in errors.into_iter().enumerate() {
            if let Some(error) = error {
                store.push(Record {
                    context: Context {
                        tick: 0,
                        round: 0,
                        player: player as u16 + 1,
                    },
                    frame: Err(error),
                });
            }
        }
        let store = Arc::new(Mutex::new(store));
        (
            Self {
                samplers,
                disabled: [false; 2],
                store: store.clone(),
            },
            store,
        )
    }

    pub fn poll(&mut self, player: usize, memory: &mut dyn Memory, tick: u32, round: u32) {
        self.store.lock().unwrap().observed = tick;
        let Some(sampler) = &mut self.samplers[player] else {
            return;
        };
        if self.disabled[player] {
            return;
        }
        let context = Context {
            tick,
            round,
            player: player as u16 + 1,
        };
        let before = sampler.capture();
        let result = sampler.sample(memory, context).map_err(bounded_error);
        let mut store = self.store.lock().unwrap();
        let error = match result {
            Ok(frame) => {
                if store.push(Record {
                    context,
                    frame: Ok(frame),
                }) {
                    None
                } else {
                    Some("telemetry pending-record limit exceeded".into())
                }
            }
            Err(error) => Some(error),
        };
        if let Some(error) = error {
            sampler.restore(&before);
            self.disabled[player] = true;
            store.push(Record {
                context,
                frame: Err(error),
            });
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            sources: std::array::from_fn(|player| {
                self.samplers[player].as_ref().map(|sampler| sampler.source().digest)
            }),
            states: Mutex::new(std::array::from_fn(|player| {
                self.samplers[player].as_ref().map(|sampler| sampler.capture())
            })),
            disabled: self.disabled,
        }
    }

    pub fn validate_snapshot(&self, snapshot: &Snapshot) -> Result<(), crate::Error> {
        let sources =
            std::array::from_fn(|player| self.samplers[player].as_ref().map(|sampler| sampler.source().digest));
        if sources != snapshot.sources {
            return Err(crate::Error::Unsupported(
                "telemetry capture belongs to different sources",
            ));
        }
        Ok(())
    }

    pub fn restore(&mut self, snapshot: &Snapshot, tick: u32) -> Result<(), crate::Error> {
        self.validate_snapshot(snapshot)?;
        let states = snapshot.states.lock().unwrap();
        for (sampler, state) in self.samplers.iter_mut().zip(states.iter()) {
            if let (Some(sampler), Some(state)) = (sampler, state) {
                sampler.restore(state);
            }
        }
        self.disabled = snapshot.disabled;
        self.store.lock().unwrap().rewind(tick);
        Ok(())
    }
}

fn bounded_error(error: Error) -> String {
    error.to_string().chars().take(4096).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct Counter {
        source: Source,
        count: u32,
    }
    impl Sampler for Counter {
        fn source(&self) -> &Source {
            &self.source
        }
        fn sample(&mut self, memory: &mut dyn Memory, context: Context) -> Result<Frame, Error> {
            self.count += 1; // The collector must roll this back even on failure.
            let mut bytes = [0];
            memory.read("main", context.player as u32 - 1, &mut bytes)?;
            Ok(Frame {
                values: [
                    ("count".into(), Value::Number(self.count as f64)),
                    ("seen".into(), Value::Number(bytes[0] as f64)),
                ]
                .into(),
                events: vec![Event {
                    name: "changed".into(),
                    fields: BTreeMap::new(),
                }],
            })
        }
    }
    struct Counters(u8);
    impl Factory for Counters {
        fn open(&self, _: &[u8], player: u16) -> Result<Option<Box<dyn Sampler>>, Error> {
            Ok(Some(Box::new(Counter {
                source: Source {
                    digest: [self.0 + player as u8; 32],
                    name: format!("source-{player}"),
                },
                count: 0,
            })))
        }
    }
    struct Bytes {
        fail: bool,
    }
    impl Memory for Bytes {
        fn read(&mut self, space: &str, address: u32, output: &mut [u8]) -> Result<(), Error> {
            assert_eq!(space, "main");
            if self.fail {
                return Err("read failed".into());
            }
            output.fill(10 + address as u8);
            Ok(())
        }
    }
    fn poll(collector: &mut Collector, tick: u32, fail: bool) {
        for player in 0..2 {
            collector.poll(player, &mut Bytes { fail }, tick, 1);
        }
    }

    #[test]
    fn checkpoints_rewind_sampler_state_and_revoke_consumed_records() {
        let (mut collector, handle) = Collector::new(&Counters(0), [&[], &[]]);
        poll(&mut collector, 1, false);
        let checkpoint = collector.snapshot();
        poll(&mut collector, 2, false);
        let first = handle.lock().unwrap().take_through(2);
        assert_eq!(first.records.len(), 4);
        assert_eq!(first.rewind_to, None);
        assert_eq!(
            first.records[2].frame.as_ref().unwrap().values["count"],
            Value::Number(2.0)
        );
        collector.restore(&checkpoint, 1).unwrap();
        poll(&mut collector, 2, false);
        let replayed = handle.lock().unwrap().take_through(2);
        assert_eq!(replayed.rewind_to, Some(1));
        assert_eq!(replayed.records, first.records[2..]);
        assert_eq!(replayed.records[0].context.player, 1);
        assert_eq!(replayed.records[1].context.player, 2);
        assert!(handle.lock().unwrap().take_through(2).records.is_empty());
    }

    #[test]
    fn speculative_failures_are_atomic_disabled_and_retry_after_restore() {
        let (mut collector, handle) = Collector::new(&Counters(0), [&[], &[]]);
        poll(&mut collector, 1, false);
        let checkpoint = collector.snapshot();
        poll(&mut collector, 2, true);
        poll(&mut collector, 3, false); // Disabled sources don't repeatedly fail.
        let first = handle.lock().unwrap().take_through(3);
        assert_eq!(first.records.len(), 4);
        assert!(first.records[2].frame.is_err() && first.records[3].frame.is_err());
        collector.restore(&checkpoint, 1).unwrap();
        poll(&mut collector, 2, false);
        let next = handle.lock().unwrap().take_through(2);
        assert_eq!(next.rewind_to, Some(1));
        for record in next.records {
            assert_eq!(record.frame.unwrap().values["count"], Value::Number(2.0));
        }
    }

    #[test]
    fn pending_records_are_truncated_and_different_sources_cannot_share_state() {
        let (mut collector, handle) = Collector::new(&Counters(0), [&[], &[]]);
        let initial = collector.snapshot();
        poll(&mut collector, 1, false);
        collector.restore(&initial, 0).unwrap();
        assert!(handle.lock().unwrap().take_through(1).records.is_empty());
        assert_eq!(handle.lock().unwrap().take_through(1).rewind_to, None);
        let (mut other, _) = Collector::new(&Counters(10), [&[], &[]]);
        assert!(other.restore(&initial, 0).is_err());
        poll(&mut collector, 1, false);
        assert_eq!(
            handle.lock().unwrap().take_through(1).records[0]
                .frame
                .as_ref()
                .unwrap()
                .values["count"],
            Value::Number(1.0)
        );
    }

    #[test]
    fn draining_releases_the_queue_budget_and_boot_failures_are_reported_once() {
        struct Failed;
        impl Factory for Failed {
            fn open(&self, _: &[u8], _: u16) -> Result<Option<Box<dyn Sampler>>, Error> {
                Err("failed to open".into())
            }
        }
        let (mut collector, handle) = Collector::new(&Failed, [&[], &[]]);
        poll(&mut collector, 1, false);
        let mut store = handle.lock().unwrap();
        let failures = store.take_through(1);
        assert_eq!(failures.records.len(), 2);
        assert!(failures
            .records
            .iter()
            .all(|record| record.context.tick == 0 && record.frame.is_err()));
        assert!(store.take_through(1).records.is_empty());
        let big = Record {
            context: Context {
                tick: 2,
                round: 1,
                player: 1,
            },
            frame: Ok(Frame {
                values: [("large".into(), Value::String("x".repeat(MAX_PENDING_BYTES / 2)))].into(),
                events: vec![],
            }),
        };
        assert!(store.push(big.clone()));
        assert!(!store.push(big.clone()));
        store.observed = 2;
        assert_eq!(store.take_through(2).records.len(), 1);
        assert_eq!(store.bytes, 0);
        assert!(store.push(big));
    }
}
