//! Bounded, lossless change-point storage for confirmed named telemetry.
//! Equal values occupy no additional history; missing values create gaps.
use std::collections::{BTreeMap, BTreeSet};

use super::{Batch, Context, Event, Frame, Record, Source, Value};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Series {
    pub player: u16,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub tick: u32,
    pub value: Option<Value>,
}

#[derive(Clone, Debug)]
pub struct Timeline {
    pub(super) extent: Option<(u32, u32)>,
    pub sources: [Option<Source>; 2],
    series: BTreeMap<Series, Vec<Point>>,
    active: [BTreeSet<String>; 2],
    events: Vec<(Context, Event)>,
    errors: Vec<(Context, String)>,
    rounds: [Vec<(u32, u32)>; 2],
    bytes: usize,
    limited_at: Option<Context>,
}

const MAX_BYTES: usize = 64 * 1024 * 1024;

impl Timeline {
    pub fn new(sources: [Option<Source>; 2]) -> Self {
        Self {
            extent: None,
            sources,
            series: BTreeMap::new(),
            active: Default::default(),
            events: Vec::new(),
            errors: Vec::new(),
            rounds: Default::default(),
            bytes: 0,
            limited_at: None,
        }
    }
    pub fn series(&self) -> &BTreeMap<Series, Vec<Point>> {
        &self.series
    }
    pub fn events(&self) -> &[(Context, Event)] {
        &self.events
    }
    pub fn errors(&self) -> &[(Context, String)] {
        &self.errors
    }
    pub fn rounds(&self) -> &[Vec<(u32, u32)>; 2] {
        &self.rounds
    }
    pub fn limited_at(&self) -> Option<Context> {
        self.limited_at
    }

    pub fn apply(&mut self, batch: Batch) {
        if let Some(tick) = batch.rewind_to {
            self.rewind(tick);
        }
        for record in batch.records {
            self.record(record);
        }
    }

    fn record(&mut self, Record { context, frame }: Record) {
        if self.limited_at.is_some() {
            return;
        }
        let Some(player) = context.player.checked_sub(1).filter(|&p| p < 2).map(usize::from) else {
            return;
        };
        let (Frame { values, events }, error) = match frame {
            Ok(frame) => (frame, None),
            Err(error) => (
                Frame {
                    values: BTreeMap::new(),
                    events: Vec::new(),
                },
                Some(error),
            ),
        };
        let mut updates = Vec::new();
        for name in &self.active[player] {
            if !values.contains_key(name) {
                updates.push((
                    Series {
                        player: context.player,
                        name: name.clone(),
                    },
                    Point {
                        tick: context.tick,
                        value: None,
                    },
                ));
            }
        }
        let active = values.keys().cloned().collect();
        for (name, value) in values {
            let key = Series {
                player: context.player,
                name,
            };
            if self
                .series
                .get(&key)
                .and_then(|points| points.last())
                .and_then(|point| point.value.as_ref())
                != Some(&value)
            {
                updates.push((
                    key,
                    Point {
                        tick: context.tick,
                        value: Some(value),
                    },
                ));
            }
        }
        let round = self.rounds[player]
            .last()
            .is_none_or(|&(_, round)| round != context.round);
        let mut extra = usize::from(round) * 32;
        for (key, point) in &updates {
            extra = extra.saturating_add(point_bytes(point));
            if !self.series.contains_key(key) {
                extra = extra.saturating_add(128 + key.name.len());
            }
        }
        for event in &events {
            extra = extra.saturating_add(event_bytes(event));
        }
        if let Some(error) = &error {
            extra = extra.saturating_add(128 + error.len());
        }
        if extra > MAX_BYTES.saturating_sub(self.bytes) {
            self.limited_at = Some(context);
            return;
        }
        self.extent = Some(
            self.extent
                .map_or((context.tick, context.tick), |(start, _)| (start, context.tick)),
        );
        self.bytes += extra;
        self.active[player] = active;
        for (key, point) in updates {
            self.series.entry(key).or_default().push(point);
        }
        if round {
            self.rounds[player].push((context.tick, context.round));
        }
        self.events.extend(events.into_iter().map(|event| (context, event)));
        if let Some(error) = error {
            self.errors.push((context, error));
        }
    }

    fn rewind(&mut self, tick: u32) {
        self.extent = self
            .extent
            .and_then(|(start, end)| (start <= tick).then_some((start, end.min(tick))));
        for points in self.series.values_mut() {
            points.truncate(points.partition_point(|point| point.tick <= tick));
        }
        self.series.retain(|_, points| !points.is_empty());
        self.active = Default::default();
        for (key, points) in &self.series {
            if points.last().is_some_and(|point| point.value.is_some()) {
                self.active[usize::from(key.player - 1)].insert(key.name.clone());
            }
        }
        self.events
            .truncate(self.events.partition_point(|(context, _)| context.tick <= tick));
        self.errors
            .truncate(self.errors.partition_point(|(context, _)| context.tick <= tick));
        for rounds in &mut self.rounds {
            rounds.truncate(rounds.partition_point(|&(at, _)| at <= tick));
        }
        if self.limited_at.is_some_and(|context| context.tick > tick) {
            self.limited_at = None;
        }
        self.bytes = self
            .series
            .iter()
            .map(|(key, points)| 128 + key.name.len() + points.iter().map(point_bytes).sum::<usize>())
            .sum::<usize>()
            + self.events.iter().map(|(_, event)| event_bytes(event)).sum::<usize>()
            + self.errors.iter().map(|(_, error)| 128 + error.len()).sum::<usize>()
            + self.rounds.iter().map(|rounds| rounds.len() * 32).sum::<usize>();
    }
}

fn point_bytes(point: &Point) -> usize {
    128 + match &point.value {
        Some(Value::String(value)) => value.len(),
        _ => 0,
    }
}
fn event_bytes(event: &Event) -> usize {
    event
        .fields
        .iter()
        .fold(128usize.saturating_add(event.name.len()), |total, (key, value)| {
            total
                .saturating_add(128)
                .saturating_add(key.len())
                .saturating_add(match value {
                    Value::String(value) => value.len(),
                    _ => 0,
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(tick: u32, value: Option<f64>) -> Record {
        Record {
            context: Context {
                tick,
                round: 1,
                player: 1,
            },
            frame: Ok(Frame {
                values: value
                    .map(|value| [("meter".into(), Value::Number(value))].into())
                    .unwrap_or_default(),
                events: Vec::new(),
            }),
        }
    }
    #[test]
    fn values_hold_gaps_and_replay_revocations_preserve_exact_steps() {
        let mut timeline = Timeline::new([None, None]);
        timeline.apply(Batch {
            rewind_to: None,
            records: vec![
                record(1, Some(7.0)),
                record(2, Some(7.0)),
                record(3, None),
                record(4, None),
                record(5, Some(9.0)),
            ],
        });
        let key = Series {
            player: 1,
            name: "meter".into(),
        };
        assert_eq!(
            timeline.series()[&key],
            vec![
                Point {
                    tick: 1,
                    value: Some(Value::Number(7.0))
                },
                Point { tick: 3, value: None },
                Point {
                    tick: 5,
                    value: Some(Value::Number(9.0))
                }
            ]
        );
        timeline.apply(Batch {
            rewind_to: Some(2),
            records: vec![record(3, Some(8.0))],
        });
        assert_eq!(
            timeline.series()[&key],
            vec![
                Point {
                    tick: 1,
                    value: Some(Value::Number(7.0))
                },
                Point {
                    tick: 3,
                    value: Some(Value::Number(8.0))
                }
            ]
        );
        timeline.apply(Batch {
            rewind_to: Some(0),
            records: vec![],
        });
        assert!(timeline.series().is_empty());
        assert!(timeline.rounds()[0].is_empty());
        assert_eq!(timeline.bytes, 0);
    }
    #[test]
    fn failure_creates_a_gap_and_rewinds_with_its_event_history() {
        let mut timeline = Timeline::new([None, None]);
        let mut first = record(1, Some(1.0));
        first.frame.as_mut().unwrap().events.push(Event {
            name: "event".into(),
            fields: BTreeMap::new(),
        });
        let mut failure = record(2, None);
        failure.frame = Err("failed".into());
        timeline.apply(Batch {
            rewind_to: None,
            records: vec![first, failure],
        });
        assert_eq!(timeline.events().len(), 1);
        assert_eq!(timeline.errors().len(), 1);
        assert_eq!(timeline.series().values().next().unwrap().last().unwrap().value, None);
        timeline.apply(Batch {
            rewind_to: Some(1),
            records: vec![record(2, Some(2.0))],
        });
        assert!(timeline.errors().is_empty());
        assert_eq!(timeline.events().len(), 1);
    }

    #[test]
    fn a_full_timeline_stops_atomically_and_a_rewind_releases_its_budget() {
        let mut timeline = Timeline::new([None, None]);
        timeline.record(record(1, Some(1.0)));
        // Simulate the rest of the history filling the shared budget.
        timeline.bytes = MAX_BYTES;
        timeline.record(record(2, None));
        assert_eq!(timeline.limited_at().unwrap().tick, 2);
        assert_eq!(timeline.active[0].len(), 1);
        assert_eq!(timeline.series().values().next().unwrap().len(), 1);
        timeline.apply(Batch {
            rewind_to: Some(1),
            records: vec![record(2, None)],
        });
        assert_eq!(timeline.limited_at(), None);
        assert!(timeline.active[0].is_empty());
        assert_eq!(timeline.series().values().next().unwrap().last().unwrap().value, None);
    }
}
