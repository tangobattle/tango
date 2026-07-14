//! The rollback loop around a [`Pair`]: local input delay, repeat-last
//! prediction for the remote side, and restore-plus-replay when a
//! prediction turns out wrong.
//!
//! Both peers construct the SAME pair (same ROM/save/RTC for player 0 and
//! player 1) and run one `Session` each, differing only in `local_player`.
//! Each frame the caller feeds the local joypad and whatever remote input
//! packets arrived, and the session keeps the pair's simulation frontier
//! one tick ahead per call, rolling back transparently as corrections
//! land. Because the pair is deterministic, two sessions fed each other's
//! outgoing packets converge on the identical state trajectory — which
//! [`checkpoint`](Session::checkpoint) digests make checkable on the wire.

use crate::{Pair, Snapshot};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Mgba(#[from] mgba::Error),
    #[error(
        "remote correction for tick {tick} arrived after the snapshot ring dropped it (ring starts at {ring_start})"
    )]
    RollbackTooDeep { tick: u32, ring_start: u32 },
    #[error("remote input for tick {tick} contradicts an already-confirmed tick")]
    NonMonotonicRemoteInput { tick: u32 },
}

/// An input packet to forward to the peer: the local player's keys,
/// scheduled for `tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outgoing {
    pub tick: u32,
    pub keys: u32,
}

/// What one [`advance`](Session::advance) did, for pacing and diagnostics.
#[derive(Debug, Clone, Copy, Default)]
pub struct Report {
    /// Ticks re-simulated due to a misprediction (0 = no rollback).
    pub rolled_back: u32,
    /// The pair's simulation frontier after this call.
    pub frontier: u32,
    /// Ticks [0, confirmed) ran with confirmed inputs on both sides and
    /// can never be rolled back again.
    pub confirmed: u32,
}

pub struct Session {
    pair: Pair,
    local_player: usize,
    delay: u32,
    /// Number of ticks simulated; the pair state is "after ticks
    /// [0, frontier)".
    frontier: u32,
    /// inputs[player][tick]: confirmed inputs. Local entries appear
    /// `delay` ticks ahead of the frontier; remote entries as packets
    /// arrive (in order).
    inputs: [Vec<Option<u32>>; 2],
    /// Count of leading remote ticks confirmed (packets are in-order, so
    /// this is the length of the Some-prefix of inputs[remote]).
    remote_confirmed: u32,
    /// The remote key value each simulated tick actually used (confirmed
    /// or predicted) — compared against corrections to decide rollback.
    remote_used: Vec<u32>,
    /// Snapshot taken BEFORE simulating tick k, for every k in
    /// [ring_start, frontier]. Bounded by `ring_cap`.
    ring: std::collections::VecDeque<(u32, Snapshot)>,
    ring_cap: usize,
    /// Earliest tick whose remote input was mispredicted, if a correction
    /// arrived; consumed by the next `advance`.
    dirty_from: Option<u32>,
    /// Ticks fully drained to the replay sink.
    drained: u32,
}

impl Session {
    /// `delay`: local inputs apply `delay` ticks after they're sampled.
    /// Both peers MUST use the same value (negotiate it like tango's
    /// per-match input delay) — ticks [0, delay) are neutral on both
    /// sides by construction, and a delay mismatch would leave a
    /// permanent hole in the peer's input schedule. `ring_cap`: how many
    /// ticks of rollback window to keep; corrections older than this are
    /// a fatal desync, so size it comfortably above worst-case latency
    /// in ticks.
    pub fn new(pair: Pair, local_player: usize, delay: u32, ring_cap: usize) -> Self {
        assert!(local_player < 2);
        assert!(ring_cap > delay as usize);
        // The first `delay` ticks of BOTH schedules are neutral: nobody's
        // sampled input can land there. Pre-confirming them keeps the
        // confirmed frontier (and replay drain) honest from tick 0.
        let prefill = vec![Some(0u32); delay as usize];
        Session {
            pair,
            local_player,
            delay,
            frontier: 0,
            inputs: [prefill.clone(), prefill],
            remote_confirmed: delay,
            remote_used: Vec::new(),
            ring: std::collections::VecDeque::new(),
            ring_cap,
            dirty_from: None,
            drained: 0,
        }
    }

    pub fn pair(&self) -> &Pair {
        &self.pair
    }

    pub fn pair_mut(&mut self) -> &mut Pair {
        &mut self.pair
    }

    pub fn local_player(&self) -> usize {
        self.local_player
    }

    /// Number of ticks simulated so far (the pair state is "after ticks
    /// [0, frontier)"). Ticks in [confirmed, frontier) are speculative.
    pub fn frontier(&self) -> u32 {
        self.frontier
    }

    fn remote_player(&self) -> usize {
        1 - self.local_player
    }

    /// Feed one remote input packet. Out-of-order or duplicate delivery of
    /// an already-confirmed tick must carry the same keys (transports
    /// should be ordered-reliable anyway).
    pub fn add_remote_input(&mut self, tick: u32, keys: u32) -> Result<(), Error> {
        let remote = self.remote_player();
        let slot = ensure_slot(&mut self.inputs[remote], tick);
        if let Some(prev) = *slot {
            if prev != keys {
                return Err(Error::NonMonotonicRemoteInput { tick });
            }
            return Ok(());
        }
        *slot = Some(keys);
        while (self.remote_confirmed as usize) < self.inputs[remote].len()
            && self.inputs[remote][self.remote_confirmed as usize].is_some()
        {
            self.remote_confirmed += 1;
        }
        // Did this correct a tick we already simulated with a prediction?
        if tick < self.frontier && self.remote_used.get(tick as usize).copied() != Some(keys) {
            self.dirty_from = Some(self.dirty_from.map_or(tick, |d| d.min(tick)));
        }
        Ok(())
    }

    /// Sample the local joypad for this frame, apply any pending
    /// correction (rolling back and re-simulating), and push the frontier
    /// forward one tick. Returns the packet to send to the peer plus a
    /// report.
    pub fn advance(&mut self, local_keys: u32) -> Result<(Outgoing, Report), Error> {
        // Schedule the local input `delay` ticks out and emit it for the
        // peer before simulating anything that could depend on it.
        let scheduled = self.frontier + self.delay;
        let local = self.local_player;
        *ensure_slot(&mut self.inputs[local], scheduled) = Some(local_keys);
        let outgoing = Outgoing {
            tick: scheduled,
            keys: local_keys,
        };

        let mut rolled_back = 0;
        if let Some(dirty) = self.dirty_from.take() {
            let ring_start = self.ring.front().map(|(t, _)| *t).unwrap_or(0);
            if dirty < ring_start {
                return Err(Error::RollbackTooDeep {
                    tick: dirty,
                    ring_start,
                });
            }
            // Restore the snapshot taken before the earliest mispredicted
            // tick and drop everything after it; re-simulation re-records.
            let idx = (dirty - ring_start) as usize;
            let (tick, snapshot) = &self.ring[idx];
            debug_assert_eq!(*tick, dirty);
            self.pair.load(snapshot)?;
            self.ring.truncate(idx + 1);
            rolled_back = self.frontier - dirty;
            self.frontier = dirty;
        }

        // Re-simulate everything the rollback unwound, plus this frame's
        // one new tick.
        let target = self.frontier + rolled_back + 1;

        while self.frontier < target {
            let t = self.frontier;
            let local_keys = self.inputs[local]
                .get(t as usize)
                .copied()
                .flatten()
                // Ticks scheduled before the session started (t < delay)
                // are neutral on both sides by construction.
                .unwrap_or(0);
            let remote_keys = self.remote_input_for(t);

            if self.ring.len() == self.ring_cap {
                self.ring.pop_front();
            }
            // Snapshot before the tick so a correction AT this tick can
            // restore to just before it. Skipped once the ring already has
            // this tick (the rollback path kept it).
            if self.ring.back().map(|(rt, _)| *rt) != Some(t) {
                self.ring.push_back((t, self.pair.save()?));
            }

            let mut keys = [0u32; 2];
            keys[local] = local_keys;
            keys[self.remote_player()] = remote_keys;
            self.pair.tick(keys);

            let slot = ensure_default(&mut self.remote_used, t);
            *slot = remote_keys;
            self.frontier += 1;
        }

        Ok((
            outgoing,
            Report {
                rolled_back,
                frontier: self.frontier,
                confirmed: self.confirmed(),
            },
        ))
    }

    fn remote_input_for(&self, tick: u32) -> u32 {
        let remote = self.remote_player();
        if let Some(Some(k)) = self.inputs[remote].get(tick as usize) {
            return *k;
        }
        // Repeat-last prediction off the newest confirmed remote input.
        if self.remote_confirmed > 0 {
            self.inputs[remote][self.remote_confirmed as usize - 1].unwrap()
        } else {
            0
        }
    }

    /// Ticks [0, confirmed) have final inputs on both sides.
    pub fn confirmed(&self) -> u32 {
        let local_confirmed = self.inputs[self.local_player]
            .iter()
            .take_while(|s| s.is_some())
            .count() as u32;
        self.frontier.min(self.remote_confirmed).min(local_confirmed)
    }

    /// The newest fully-confirmed tick whose pre-tick snapshot is still in
    /// the ring, digested for cross-peer desync detection: both peers
    /// eventually produce a checkpoint for the same tick, and the digests
    /// must match bit for bit.
    pub fn checkpoint(&self) -> Option<(u32, u32)> {
        // A correction that hasn't been applied yet (rollback runs on the
        // next advance) means ring snapshots past the dirty tick are stale
        // speculation — don't vouch for them.
        let confirmed = self.confirmed().min(self.dirty_from.unwrap_or(u32::MAX));
        self.ring
            .iter()
            .rev()
            .find(|(t, _)| *t <= confirmed)
            .map(|(t, snap)| (*t, snap.digest()))
    }

    /// Digest of the ring snapshot at exactly `tick`, if it's still in the
    /// ring and within the trustworthy confirmed range — the receive side
    /// of cross-peer desync checking (compare against a peer's
    /// [`checkpoint`](Self::checkpoint)). None means "can't check this
    /// one", not "mismatch".
    pub fn digest_at(&self, tick: u32) -> Option<u32> {
        let confirmed = self.confirmed().min(self.dirty_from.unwrap_or(u32::MAX));
        if tick > confirmed {
            return None;
        }
        self.ring.iter().find(|(t, _)| *t == tick).map(|(_, s)| s.digest())
    }

    /// Drain newly-confirmed ticks as (tick, [p0 keys, p1 keys]) — final
    /// input pairs in tick order, suitable for a replay sink.
    pub fn drain_confirmed(&mut self) -> Vec<(u32, [u32; 2])> {
        let confirmed = self.confirmed();
        let mut out = Vec::new();
        while self.drained < confirmed {
            let t = self.drained as usize;
            out.push((self.drained, [self.inputs[0][t].unwrap(), self.inputs[1][t].unwrap()]));
            self.drained += 1;
        }
        out
    }
}

fn ensure_slot(v: &mut Vec<Option<u32>>, tick: u32) -> &mut Option<u32> {
    let idx = tick as usize;
    if v.len() <= idx {
        v.resize(idx + 1, None);
    }
    &mut v[idx]
}

fn ensure_default(v: &mut Vec<u32>, tick: u32) -> &mut u32 {
    let idx = tick as usize;
    if v.len() <= idx {
        v.resize(idx + 1, 0);
    }
    &mut v[idx]
}
