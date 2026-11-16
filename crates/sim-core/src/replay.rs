//! Recording and replaying matches.
//!
//! Because the sim is deterministic, a whole match is reproduced by its seed,
//! the two species, and the intents sent on every tick. That's a few bytes per
//! tick instead of a full state snapshot. `record` plays a match with
//! controllers and keeps those inputs; `play` feeds them back through `step`.
//! Comparing the per-tick hashes of two runs shows whether they match, and if
//! not, the first tick where they diverged.

use crate::controller::{Controller, Rush};
use crate::data::{BLASTOISE, PIKACHU, SpeciesId};
use crate::event::Event;
use crate::hash::{combine, hash_state};
use crate::state::{Intent, State};
use crate::step::step;

/// Everything needed to reproduce a match exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    pub seed: u64,
    pub species: [SpeciesId; 2],
    /// `intents[t]` is what both fighters sent on tick `t`.
    pub intents: Vec<[Intent; 2]>,
}

/// What a run produced, for comparing two runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    /// The state after the last tick.
    pub state: State,
    /// `hashes[t]` is the state hash after tick `t`.
    pub hashes: Vec<u64>,
    /// Every event, with the tick it happened on.
    pub events: Vec<(u32, Event)>,
}

impl Trace {
    fn start(state: State) -> Self {
        Self {
            state,
            hashes: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Steps once and records the result.
    fn step(&mut self, intents: [Intent; 2]) {
        let tick = self.state.tick;
        for event in step(&mut self.state, intents) {
            self.events.push((tick, event));
        }
        self.hashes.push(hash_state(&self.state));
    }

    /// One hash for the whole run.
    pub fn hash(&self) -> u64 {
        combine(&self.hashes)
    }
}

/// Plays a full match with two controllers, keeping their inputs.
///
/// `controllers[i]` drives fighter `i`. They're `&mut dyn Controller` (trait
/// objects) so the two can be different types, such as a keyboard against an
/// AI. Runs until the match ends, which the match timer guarantees.
pub fn record(
    seed: u64,
    species: [SpeciesId; 2],
    controllers: [&mut dyn Controller; 2],
) -> (Replay, Trace) {
    let [c0, c1] = controllers;
    let mut trace = Trace::start(State::new(seed, species));
    let mut intents = Vec::new();
    while trace.state.outcome.is_none() {
        let tick_intents = [c0.intent(&trace.state, 0), c1.intent(&trace.state, 1)];
        trace.step(tick_intents);
        intents.push(tick_intents);
    }
    let replay = Replay {
        seed,
        species,
        intents,
    };
    (replay, trace)
}

/// Re-runs a recorded match from its inputs alone.
pub fn play(replay: &Replay) -> Trace {
    let mut trace = Trace::start(State::new(replay.seed, replay.species));
    for &intents in &replay.intents {
        trace.step(intents);
    }
    trace
}

/// A fixed, fully scripted match that exercises every mechanic.
///
/// Its hash is pinned in `REFERENCE_MATCH_HASH`. Any build that computes the
/// same value plays the game identically: the native test checks it here, and
/// the WASM check runs the same function compiled to WebAssembly.
pub fn reference_match() -> Trace {
    let mut a = Rush::new(1);
    let mut b = Rush::new(2);
    record(2026, [PIKACHU, BLASTOISE], [&mut a, &mut b]).1
}

/// The hash `reference_match` must produce.
///
/// It changes whenever gameplay changes: a balance tweak or a rule change
/// plays the reference match differently. That's expected. Check the change
/// was intended, then update this value (the failing test prints the new
/// one). It must never change from refactoring alone.
pub const REFERENCE_MATCH_HASH: u64 = 0x1874_5be5_2053_5896;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::{Idle, Random};
    use crate::data::AQUA_JET;
    use crate::state::Outcome;
    use crate::step::MATCH_TICKS;

    #[test]
    fn replay_reproduces_the_recorded_match() {
        let mut a = Rush::new(5);
        let mut b = Random::new(6);
        let (replay, recorded) = record(9, [PIKACHU, BLASTOISE], [&mut a, &mut b]);
        assert_eq!(play(&replay), recorded);
    }

    #[test]
    fn many_seeds_replay_identically() {
        for seed in 0..25 {
            let mut a = Rush::new(seed);
            let mut b = Rush::new(seed + 100);
            let (replay, recorded) = record(seed, [PIKACHU, BLASTOISE], [&mut a, &mut b]);
            let replayed = play(&replay);
            assert_eq!(
                replayed.hashes, recorded.hashes,
                "seed {seed} diverged on replay"
            );
        }
    }

    #[test]
    fn different_inputs_give_different_runs() {
        let mut a = Rush::new(1);
        let mut b = Rush::new(2);
        let first = record(1, [PIKACHU, BLASTOISE], [&mut a, &mut b]).1;
        let mut a = Rush::new(3);
        let mut b = Rush::new(4);
        let second = record(1, [PIKACHU, BLASTOISE], [&mut a, &mut b]).1;
        assert_ne!(first.hash(), second.hash());
    }

    #[test]
    fn idle_match_ends_on_time() {
        let (replay, trace) = record(1, [PIKACHU, BLASTOISE], [&mut Idle, &mut Idle]);
        assert_eq!(replay.intents.len(), MATCH_TICKS as usize);
        assert_eq!(trace.state.outcome, Some(Outcome::Draw));
    }

    #[test]
    fn reference_match_exercises_every_mechanic() {
        let trace = reference_match();
        let happened = |pred: fn(&Event) -> bool| trace.events.iter().any(|(_, e)| pred(e));
        assert!(happened(|e| matches!(e, Event::Dashed { .. })));
        assert!(happened(|e| matches!(e, Event::ProjectileSpawned { .. })));
        assert!(happened(|e| matches!(e, Event::TelegraphStarted { .. })));
        assert!(happened(|e| matches!(e, Event::ZoneLanded { .. })));
        assert!(happened(|e| matches!(e, Event::Hit { .. })));
        assert!(happened(|e| matches!(
            e,
            Event::Hit {
                move_id: AQUA_JET,
                ..
            }
        )));
        assert!(happened(|e| matches!(e, Event::Fainted { .. })));
        assert!(happened(|e| matches!(e, Event::MatchEnded { .. })));
    }

    #[test]
    fn reference_match_hash_is_pinned() {
        let actual = reference_match().hash();
        assert_eq!(
            actual, REFERENCE_MATCH_HASH,
            "gameplay changed: if intended, set REFERENCE_MATCH_HASH to {actual:#018x}"
        );
    }
}
