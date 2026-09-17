//! Replay workflow state, independent of screens and application resources.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[derive(Default)]
pub struct Controller {
    pending: Option<PathBuf>,
    carry_speed: Option<f32>,
    was_playing: bool,
    jobs: HashMap<PathBuf, (Arc<AtomicBool>, iced::task::Handle)>,
}
impl Controller {
    pub fn defer(&mut self, path: PathBuf) {
        self.pending = Some(path);
    }
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn take_pending(&mut self) -> Option<PathBuf> {
        self.pending.take()
    }
    pub fn cancel_pending(&mut self) {
        self.pending = None;
        self.carry_speed = None;
    }
    pub fn take_speed(&mut self) -> Option<f32> {
        self.carry_speed.take()
    }
    pub fn handoff(&mut self, speed: Option<f32>) {
        self.carry_speed = speed;
        self.was_playing = false;
    }
    pub fn observe(&mut self, playing: bool, seeking: bool, tick: u32, total: u32) -> bool {
        let was_playing = std::mem::replace(&mut self.was_playing, playing);
        was_playing && !seeking && tick >= total
    }
    pub fn reset_playback(&mut self) {
        self.was_playing = false;
    }
    pub fn track(&mut self, path: PathBuf, cancel: Arc<AtomicBool>, handle: iced::task::Handle) {
        self.takeover(&path);
        self.jobs.insert(path, (cancel, handle));
    }
    pub fn finished(&mut self, path: &Path) {
        self.jobs.remove(path);
    }
    pub fn takeover(&mut self, path: &Path) {
        if let Some((cancel, handle)) = self.jobs.remove(path) {
            cancel.store(true, Ordering::Relaxed);
            handle.abort();
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        for (cancel, handle) in self.jobs.values() {
            cancel.store(true, Ordering::Relaxed);
            handle.abort();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_advances_after_playback_but_not_a_paused_seek() {
        let mut owner = Controller::default();
        assert!(!owner.observe(false, false, 100, 100));
        assert!(!owner.observe(true, false, 50, 100));
        assert!(!owner.observe(false, true, 100, 100));
        assert!(!owner.observe(false, false, 100, 100));
        owner.observe(true, false, 99, 100);
        assert!(owner.observe(false, false, 100, 100));
    }
    #[test]
    fn download_cancellation_clears_deferred_queue_speed() {
        let mut owner = Controller::default();
        owner.handoff(Some(2.0));
        owner.defer("replay".into());
        owner.cancel_pending();
        assert!(!owner.has_pending());
        assert_eq!(owner.take_speed(), None);
    }
}
