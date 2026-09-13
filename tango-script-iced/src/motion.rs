//! Per-renderer presentation clocks. No script callbacks run on animation frames.
use std::collections::{BTreeMap, BTreeSet};

use iced::{animation::Animation, time::Instant};
use tango_script::ui::{motion::Easing, Motion, Node};

#[derive(Default)]
pub(super) struct Timelines(BTreeMap<String, Timeline>);

struct Timeline {
    descriptor: Motion,
    animation: Animation<f32>,
}

#[derive(Default)]
pub(super) struct Frame(pub BTreeMap<String, Option<f32>>);

impl Frame {
    pub fn active(&self) -> bool {
        self.0.values().any(Option::is_some)
    }
}

impl Timelines {
    pub fn sample(&mut self, node: &Node, now: Instant) -> Frame {
        let mut seen = BTreeSet::new();
        let mut frame = Frame::default();
        self.visit(node, now, &mut seen, &mut frame);
        // Only descriptors in the current tree survive: arbitrary revision/key
        // changes cannot accumulate clocks across updates or document reloads.
        self.0.retain(|key, _| seen.contains(key));
        frame
    }

    fn visit(&mut self, node: &Node, now: Instant, seen: &mut BTreeSet<String>, frame: &mut Frame) {
        if let Some(motion) = &node.motion {
            seen.insert(motion.key.clone());
            let timeline = self
                .0
                .entry(motion.key.clone())
                .or_insert_with(|| Timeline::new(motion, now));
            if &timeline.descriptor != motion {
                *timeline = Timeline::new(motion, now);
            }
            frame.0.insert(
                motion.key.clone(),
                timeline
                    .animation
                    .is_animating(now)
                    .then(|| timeline.animation.interpolate_with(|v| v, now)),
            );
        }
        for child in node.children() {
            self.visit(child, now, seen, frame);
        }
    }
}

impl Timeline {
    fn new(motion: &Motion, now: Instant) -> Self {
        use iced::animation::Easing as Native;
        let easing = match motion.easing {
            Easing::Linear => Native::Linear,
            Easing::EaseInCubic => Native::EaseInCubic,
            Easing::EaseOutCubic => Native::EaseOutCubic,
            Easing::EaseInOutCubic => Native::EaseInOutCubic,
        };
        let animation = if motion.duration_ms == 0 {
            Animation::new(1.0)
        } else {
            Animation::new(0.0)
                .duration(std::time::Duration::from_millis(motion.duration_ms.into()))
                .delay(std::time::Duration::from_millis(motion.delay_ms.into()))
                .easing(easing)
                .go(1.0, now)
        };
        Self {
            descriptor: motion.clone(),
            animation,
        }
    }
}

pub(super) fn apply<'a>(
    body: iced::Element<'a, super::Message>,
    motion: Option<&Motion>,
    frame: &Frame,
) -> iced::Element<'a, super::Message> {
    let Some(motion) = motion else { return body };
    let progress = frame.0.get(&motion.key).copied().flatten();
    tango_ui::anim::slide_in_opt(body, progress, iced::Vector::new(motion.from[0], motion.from[1]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn node() -> Node {
        serde_json::from_str(r#"{"kind":"space","motion":{"key":"pane","revision":"0","from":[0,20]}}"#).unwrap()
    }

    #[test]
    fn entrance_timing_matches_native_and_rebuilds_do_not_restart_it() {
        let now = Instant::now();
        let mut timelines = Timelines::default();
        let mut node = node();
        node.motion.as_mut().unwrap().delay_ms = 40;
        let mut native = tango_ui::anim::Enter::default();
        native.start_delayed(now, Duration::from_millis(40));
        for ms in [0, 20, 40, 41, 80, 120, 160, 199, 200, 201, 1000] {
            let now = now + Duration::from_millis(ms);
            let frame = timelines.sample(&node.clone(), now);
            assert_eq!(frame.0["pane"], native.progress(now), "{ms}");
        }
        let later = now + Duration::from_secs(2);
        node.motion.as_mut().unwrap().revision = "1".into();
        assert_eq!(timelines.sample(&node, later).0["pane"], Some(0.0));
        assert!(timelines.sample(&node, later + Duration::from_millis(100)).0["pane"].unwrap() > 0.0);
        node.motion.as_mut().unwrap().revision = "2".into();
        assert_eq!(
            timelines.sample(&node, later + Duration::from_millis(101)).0["pane"],
            Some(0.0)
        );
    }

    #[test]
    fn clocks_are_scoped_to_a_renderer_and_removed_descriptors_are_evicted() {
        let now = Instant::now();
        let mut one = Timelines::default();
        let mut two = Timelines::default();
        let mut node = node();
        one.sample(&node, now);
        assert_eq!(one.sample(&node, now + Duration::from_secs(1)).0["pane"], None);
        assert_eq!(two.sample(&node, now + Duration::from_secs(1)).0["pane"], Some(0.0));
        for index in 0..100 {
            node.motion.as_mut().unwrap().key = index.to_string();
            one.sample(&node, now + Duration::from_secs(2));
            assert_eq!(one.0.len(), 1);
        }
        let descriptor = node.motion.take();
        assert!(!one.sample(&node, now + Duration::from_secs(3)).active());
        assert!(one.0.is_empty());
        node.motion = descriptor;
        assert_eq!(one.sample(&node, now + Duration::from_secs(4)).0["99"], Some(0.0));
        node.motion.as_mut().unwrap().duration_ms = 0;
        assert!(!one.sample(&node, now + Duration::from_secs(5)).active());
    }
}
