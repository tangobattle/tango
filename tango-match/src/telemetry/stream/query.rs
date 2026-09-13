//! Read-only, bounded-resolution queries over exact telemetry history.
use super::{Series, Timeline, Value};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub player: u16,
    pub name: String,
    pub from_tick: u32,
    pub to_tick: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValueAt {
    pub player: u16,
    pub name: String,
    pub tick: u32,
}
impl Range {
    pub fn validate(&self) -> Result<(), super::Error> {
        if !(1..=2).contains(&self.player)
            || self.name.is_empty()
            || self.name.len() > 4096
            || self.from_tick > self.to_tick
        {
            return Err("invalid telemetry range".into());
        }
        Ok(())
    }
}

/// A numeric envelope over an inclusive interval. Extremes retain short spikes;
/// a gap forbids joining this interval to its neighbors with a continuous line.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Bucket {
    pub from_tick: u32,
    pub to_tick: u32,
    pub first: Option<f64>,
    pub last: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub gap: bool,
}

impl Timeline {
    pub fn value_at(&self, query: &ValueAt) -> Result<Option<&Value>, super::Error> {
        Range {
            player: query.player,
            name: query.name.clone(),
            from_tick: query.tick,
            to_tick: query.tick,
        }
        .validate()?;
        if self
            .extent()
            .is_none_or(|(start, end)| query.tick < start || query.tick > end)
        {
            return Ok(None);
        }
        let Some(points) = self.series().get(&Series {
            player: query.player,
            name: query.name.clone(),
        }) else {
            return Ok(None);
        };
        let end = points.partition_point(|point| point.tick <= query.tick);
        Ok(end.checked_sub(1).and_then(|index| points[index].value.as_ref()))
    }
    pub fn numeric_series(&self, range: &Range, buckets: usize) -> Result<Vec<Bucket>, super::Error> {
        range.validate()?;
        if !(1..=2048).contains(&buckets) {
            return Err("telemetry resolution must be in 1..=2048".into());
        }
        let points = self
            .series()
            .get(&Series {
                player: range.player,
                name: range.name.clone(),
            })
            .map(Vec::as_slice)
            .unwrap_or_default();
        let span = u64::from(range.to_tick) - u64::from(range.from_tick) + 1;
        let count = (buckets as u64).min(span);
        let mut index = points.partition_point(|p| p.tick <= range.from_tick);
        let number = |value: Option<&Value>| match value {
            Some(Value::Number(n)) if n.is_finite() => Some(*n),
            _ => None,
        };
        let mut current = index.checked_sub(1).and_then(|i| number(points[i].value.as_ref()));
        let mut output = Vec::with_capacity(count as usize);
        for bucket in 0..count {
            let start = u64::from(range.from_tick) + bucket * span / count;
            let end = u64::from(range.from_tick) + (bucket + 1) * span / count;
            // Changes exactly at the boundary belong to this bucket only.
            while index < points.len() && u64::from(points[index].tick) <= start {
                current = number(points[index].value.as_ref());
                index += 1;
            }
            if self.extent().is_none_or(|(_, last)| start > u64::from(last)) {
                current = None;
            }
            let mut result = Bucket {
                from_tick: start as u32,
                to_tick: (end - 1) as u32,
                first: current,
                last: current,
                min: current,
                max: current,
                gap: current.is_none(),
            };
            while index < points.len() && u64::from(points[index].tick) < end {
                current = number(points[index].value.as_ref());
                index += 1;
                if let Some(value) = current {
                    result.min = Some(result.min.map_or(value, |n| n.min(value)));
                    result.max = Some(result.max.map_or(value, |n| n.max(value)));
                } else {
                    result.gap = true;
                }
            }
            if self.extent().is_none_or(|(_, last)| end - 1 > u64::from(last)) {
                current = None;
                result.gap = true;
            }
            result.last = current;
            output.push(result);
        }
        Ok(output)
    }

    pub fn count_events(&self, range: &Range) -> Result<usize, super::Error> {
        range.validate()?;
        let events = self.events();
        let start = events.partition_point(|(context, _)| context.tick < range.from_tick);
        let end = events.partition_point(|(context, _)| context.tick <= range.to_tick);
        Ok(events[start..end]
            .iter()
            .filter(|(context, event)| context.player == range.player && event.name == range.name)
            .count())
    }

    /// Inclusive extent of collected history, including constant-value tails.
    pub fn extent(&self) -> Option<(u32, u32)> {
        self.extent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::stream::{Batch, Context, Frame, Record};
    #[test]
    fn envelopes_preserve_spikes_gaps_and_exact_bucket_boundaries() {
        let mut timeline = Timeline::new([None, None]);
        timeline.apply(Batch {
            rewind_to: None,
            records: [
                (1, Some(10.)),
                (2, Some(100.)),
                (3, Some(10.)),
                (4, None),
                (5, Some(20.)),
                (8, Some(30.)),
                (9, Some(30.)),
            ]
            .into_iter()
            .map(|(tick, value)| Record {
                context: Context {
                    tick,
                    round: 1,
                    player: 1,
                },
                frame: Ok(Frame {
                    values: value
                        .map(|n| [("n".into(), Value::Number(n))].into())
                        .unwrap_or_default(),
                    events: vec![],
                }),
            })
            .collect(),
        });
        let range = Range {
            player: 1,
            name: "n".into(),
            from_tick: 0,
            to_tick: 9,
        };
        let buckets = timeline.numeric_series(&range, 2).unwrap();
        assert_eq!(
            buckets[0],
            Bucket {
                from_tick: 0,
                to_tick: 4,
                first: None,
                last: None,
                min: Some(10.),
                max: Some(100.),
                gap: true
            }
        );
        assert_eq!(
            buckets[1],
            Bucket {
                from_tick: 5,
                to_tick: 9,
                first: Some(20.),
                last: Some(30.),
                min: Some(20.),
                max: Some(30.),
                gap: false
            }
        );
        assert_eq!(timeline.extent(), Some((1, 9)));
        assert!(timeline.numeric_series(&range, 0).is_err());
        let mut end = range.clone();
        end.from_tick = u32::MAX;
        end.to_tick = u32::MAX;
        assert_eq!(timeline.numeric_series(&end, 2048).unwrap()[0].last, None);
        timeline.apply(Batch {
            rewind_to: Some(3),
            records: vec![],
        });
        assert_eq!(timeline.extent(), Some((1, 3)));
    }
}
