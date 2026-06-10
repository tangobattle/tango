//! Concurrent driver for the shadow co-sim on the live PvP path.
//!
//! Each [`Stepper`](crate::stepper::Stepper) step used to run the shadow's
//! tick synchronously inside the primary's tick body (the per-game
//! `copy_input_data_entry` trap resolved the remote packet by driving the
//! shadow core to completion), so every simulated frame paid for two full
//! GBA ticks back to back. But the packet the trap needs is buffered by the
//! shadow's *previous* run ([`Shadow::begin_apply_input`]) — the current run
//! only produces the *next* tick's packet. So [`Worker`] hands the packet
//! back immediately and completes the shadow's tick
//! ([`Shadow::finish_apply_input`]) on a dedicated thread, overlapping it
//! with the rest of the primary's tick (and, in a rollback re-sim, with the
//! primary's *next* tick — the steps pipeline).
//!
//! Submission happens from inside the consuming trap — after every
//! round-end gate — never speculatively at step start, so a tick whose
//! exchange the game skips (round ending, post-round-end frames) simply
//! never queues a run and the shadow stays parked, exactly as in the
//! synchronous version. The simulation is byte-identical: same packet,
//! same shadow execution, only its wall-clock placement moves.
//!
//! Join discipline: at most one run is ever in flight, and every other
//! access to the shadow — the next [`resolve`](Worker::resolve), a
//! [`save_state_reusing`](Worker::save_state_reusing) /
//! [`load_state`](Worker::load_state) from the rollback engine, or `Drop`
//! (the match's round-end path locks the shared shadow right after the
//! Round, and with it this worker, is dropped) — waits for it first via
//! [`join_pending`](Worker::join_pending), which also surfaces the deferred
//! run's error. Replay mode keeps the synchronous
//! [`Shadow::apply_input`] resolver and never touches this.

use std::sync::{Arc, Mutex};

use crate::input::{Input, PartialInput};

use super::{Shadow, ShadowSnapshot};

/// One queued "finish the shadow's tick" request. `done` reports the run's
/// outcome back to whoever joins it.
struct Job {
    done: std::sync::mpsc::Sender<anyhow::Result<()>>,
}

/// See the [module docs](self).
pub struct Worker {
    shadow: Arc<Mutex<Shadow>>,
    /// Job submission side; `None` once `Drop` disconnects it (which is what
    /// ends the worker thread's receive loop).
    jobs: Option<std::sync::mpsc::Sender<Job>>,
    /// Completion channel of the (at most one) in-flight run.
    inflight: Mutex<Option<std::sync::mpsc::Receiver<anyhow::Result<()>>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    pub fn new(shadow: Arc<Mutex<Shadow>>) -> Self {
        let (jobs, job_rx) = std::sync::mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
            .name("tango shadow worker".to_string())
            .spawn({
                let shadow = shadow.clone();
                move || {
                    while let Ok(job) = job_rx.recv() {
                        let result = shadow.lock().unwrap().finish_apply_input();
                        // A dropped receiver just means nobody is waiting yet;
                        // the result sits in the channel for join_pending.
                        let _ = job.done.send(result);
                    }
                }
            })
            .expect("spawn shadow worker");
        Self {
            shadow,
            jobs: Some(jobs),
            inflight: Mutex::new(None),
            thread: Some(thread),
        }
    }

    /// Wait out the in-flight shadow run, if any, surfacing its error. After
    /// this returns Ok the shadow is parked at its boundary and safe to
    /// snapshot, rewind, or run again.
    pub fn join_pending(&self) -> anyhow::Result<()> {
        let inflight = self.inflight.lock().unwrap().take();
        if let Some(done) = inflight {
            done.recv().map_err(|_| anyhow::format_err!("shadow worker is gone"))??;
        }
        Ok(())
    }

    /// Snapshot the shadow at the boundary its last run parked it at,
    /// joining the in-flight run first.
    pub fn save_state_reusing(
        &self,
        buf: Box<std::mem::MaybeUninit<mgba::state::State>>,
    ) -> anyhow::Result<ShadowSnapshot> {
        self.join_pending()?;
        self.shadow.lock().unwrap().save_state_reusing(buf)
    }

    /// Rewind the shadow to `snapshot` before a rollback re-sim, joining the
    /// in-flight run first so it can't land on top of the restored state.
    pub fn load_state(&self, snapshot: &ShadowSnapshot) -> anyhow::Result<()> {
        self.join_pending()?;
        self.shadow.lock().unwrap().load_state(snapshot)
    }
}

impl crate::stepper::RemotePacketSource for Worker {
    fn resolve(&self, _tick: u32, pair: (Input, PartialInput)) -> anyhow::Result<Vec<u8>> {
        // The previous tick's run must have completed: it buffered the packet
        // the peek below returns and parked the core at the boundary this
        // tick's run continues from. (In steady state it finished long ago —
        // it had the rest of the primary's previous tick to itself.)
        self.join_pending()?;
        let packet = self.shadow.lock().unwrap().begin_apply_input(pair)?;
        let (done, done_rx) = std::sync::mpsc::channel();
        self.jobs
            .as_ref()
            .expect("jobs channel lives until Drop")
            .send(Job { done })
            .map_err(|_| anyhow::format_err!("shadow worker is gone"))?;
        *self.inflight.lock().unwrap() = Some(done_rx);
        Ok(packet)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // The match's round-end path loads + advances the shared shadow right
        // after dropping the Round (and with it this worker); an in-flight run
        // racing that would corrupt it, so wait it out. Its error, if any, no
        // longer has anywhere to go.
        let _ = self.join_pending();
        // Disconnect the job channel so the worker loop's recv() errors out.
        self.jobs = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
