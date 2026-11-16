//! Controllers decide what a fighter wants to do each tick.
//!
//! Anything that drives a fighter implements `Controller`: the keyboard in the
//! debug viewer, scripted test opponents, and later AI. `step` never knows
//! which kind is playing; it only sees the `Intent`s they produce.

use crate::math::{Dir8, Vec2};
use crate::rng::Rng;
use crate::state::{Intent, State};

/// Produces one fighter's `Intent` for the coming tick.
///
/// Takes `&State` (read-only), so a controller can look at the match but
/// never change it. Takes `&mut self` so a controller can keep its own memory
/// between ticks, such as a position in a script or its own RNG.
pub trait Controller {
    /// `me` is the index of the fighter this controller drives.
    fn intent(&mut self, state: &State, me: usize) -> Intent;
}

/// Does nothing, every tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct Idle;

impl Controller for Idle {
    fn intent(&mut self, _state: &State, _me: usize) -> Intent {
        Intent::default()
    }
}

/// Plays a fixed list of intents, one per tick, then does nothing.
#[derive(Clone, Debug, Default)]
pub struct Scripted {
    intents: Vec<Intent>,
    next: usize,
}

impl Scripted {
    pub fn new(intents: Vec<Intent>) -> Self {
        Self { intents, next: 0 }
    }
}

impl Controller for Scripted {
    fn intent(&mut self, _state: &State, _me: usize) -> Intent {
        let intent = self.intents.get(self.next).copied().unwrap_or_default();
        self.next += 1;
        intent
    }
}

/// Mashes random inputs, for stress-testing the sim.
///
/// It has its own seeded RNG and never touches `State::rng`: a controller
/// changing the match's RNG would make the match depend on who is playing,
/// not just on what they press.
#[derive(Clone, Debug)]
pub struct Random {
    rng: Rng,
}

impl Random {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
        }
    }
}

impl Controller for Random {
    fn intent(&mut self, _state: &State, _me: usize) -> Intent {
        // 9 choices: the 8 directions or standing still.
        let movement = Dir8::ALL.get(self.rng.below(9) as usize).copied();
        Intent {
            movement,
            dash: self.rng.chance(1, 20),
            // Slot 2 doesn't exist, so the "bad slot is ignored" path gets
            // exercised too.
            move_slot: self.rng.chance(1, 8).then(|| self.rng.below(3) as u8),
        }
    }
}

/// Walks at the opponent and attacks at random, for test matches that actually
/// fight: hits, knockouts, lunges and zones all happen.
///
/// Like `Random`, it uses its own RNG, never `State::rng`.
#[derive(Clone, Debug)]
pub struct Rush {
    rng: Rng,
}

impl Rush {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
        }
    }
}

impl Controller for Rush {
    fn intent(&mut self, state: &State, me: usize) -> Intent {
        let to_opponent = state.fighters[1 - me].pos - state.fighters[me].pos;
        // Close in until roughly touching, then circle at random.
        let movement = if to_opponent.length() > 1_500 {
            toward(to_opponent)
        } else {
            Dir8::ALL.get(self.rng.below(9) as usize).copied()
        };
        Intent {
            movement,
            dash: self.rng.chance(1, 30),
            move_slot: self.rng.chance(1, 6).then(|| self.rng.below(2) as u8),
        }
    }
}

/// The 8-way direction closest to `v`, or `None` for a zero vector. An axis
/// counts only if it's at least half as long as the other, so a mostly
/// horizontal vector maps to E or W, not a diagonal.
fn toward(v: Vec2) -> Option<Dir8> {
    let sign = |a: i32, b: i32| {
        if a.abs() * 2 >= b.abs() {
            a.signum()
        } else {
            0
        }
    };
    match (sign(v.x, v.y), sign(v.y, v.x)) {
        (0, -1) => Some(Dir8::N),
        (1, -1) => Some(Dir8::NE),
        (1, 0) => Some(Dir8::E),
        (1, 1) => Some(Dir8::SE),
        (0, 1) => Some(Dir8::S),
        (-1, 1) => Some(Dir8::SW),
        (-1, 0) => Some(Dir8::W),
        (-1, -1) => Some(Dir8::NW),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{BLASTOISE, PIKACHU};

    #[test]
    fn toward_picks_the_nearest_of_eight_directions() {
        assert_eq!(toward(Vec2::new(1_000, 0)), Some(Dir8::E));
        assert_eq!(toward(Vec2::new(1_000, 100)), Some(Dir8::E));
        assert_eq!(toward(Vec2::new(1_000, 900)), Some(Dir8::SE));
        assert_eq!(toward(Vec2::new(-5, -1_000)), Some(Dir8::N));
        assert_eq!(toward(Vec2::ZERO), None);
    }

    #[test]
    fn rush_walks_at_a_distant_opponent() {
        // Pikachu starts due west of Blastoise.
        let state = new_match();
        assert_eq!(Rush::new(3).intent(&state, 0).movement, Some(Dir8::E));
        assert_eq!(Rush::new(3).intent(&state, 1).movement, Some(Dir8::W));
    }

    fn new_match() -> State {
        State::new(1, [PIKACHU, BLASTOISE])
    }

    #[test]
    fn idle_does_nothing() {
        assert_eq!(Idle.intent(&new_match(), 0), Intent::default());
    }

    #[test]
    fn scripted_plays_its_list_then_idles() {
        let dash = Intent {
            dash: true,
            ..Intent::default()
        };
        let mut c = Scripted::new(vec![dash, Intent::default(), dash]);
        let state = new_match();
        let played: Vec<Intent> = (0..5).map(|_| c.intent(&state, 0)).collect();
        assert_eq!(
            played,
            vec![
                dash,
                Intent::default(),
                dash,
                Intent::default(),
                Intent::default()
            ]
        );
    }

    #[test]
    fn random_is_repeatable_for_the_same_seed() {
        let state = new_match();
        let mut a = Random::new(7);
        let mut b = Random::new(7);
        for _ in 0..200 {
            assert_eq!(a.intent(&state, 0), b.intent(&state, 0));
        }
    }
}
