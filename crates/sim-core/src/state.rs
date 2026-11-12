//! Everything that changes during a match, plus per-tick inputs.
//!
//! `State` is one snapshot of the match. `Intent` is what one controller wants
//! this tick; `step` combines them into the next `State`.
//!
//! Entities live in `Vec`s and arrays and refer to each other by index, never
//! by reference. That keeps the borrow checker happy and the state hashable.
//! Indices use fixed-width integers (`u8`, `u16`), not `usize`, so the state
//! hashes the same on 64-bit native and 32-bit WASM builds.
//!
//! Distances in units, durations in ticks.

use crate::data::{MoveId, SpeciesId};
use crate::math::{Dir8, Vec2};
use crate::rng::Rng;

/// Half-width and half-height of the arena: 24 x 16 tiles. Placeholder to tune
/// in the viewer; arena size is still open (DESIGN.md §9).
pub const ARENA_HALF_EXTENTS: Vec2 = Vec2::new(12_000, 8_000);
/// Each fighter starts this far from the center on the x axis (12 tiles apart).
pub const SPAWN_OFFSET: i32 = 6_000;

/// One fighter's input for one tick. Mirrors the brain's action space.
/// `Intent::default()` means "do nothing".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Intent {
    /// `None` = stand still.
    pub movement: Option<Dir8>,
    pub dash: bool,
    /// Index into the species' `moves`. `None` = don't cast.
    pub move_slot: Option<u8>,
}

/// What a fighter is busy doing. Only `Idle` accepts new commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Idle,
    /// Universal movement dash. `velocity` is per tick.
    Dashing {
        velocity: Vec2,
        ticks_left: u16,
    },
    /// Windup before the move in `slot` is released.
    Casting {
        slot: u8,
        ticks_left: u16,
    },
    /// After a move, before the fighter can act again.
    Recovering {
        ticks_left: u16,
    },
    /// A dash-attack move in flight. `has_hit` stops it hitting twice.
    Lunging {
        slot: u8,
        velocity: Vec2,
        ticks_left: u16,
        has_hit: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fighter {
    pub species: SpeciesId,
    pub pos: Vec2,
    pub facing: Dir8,
    pub hp: i32,
    pub max_hp: i32,
    /// Ticks until each move slot can be used again (0 = ready).
    pub move_cooldowns: [u16; 2],
    /// Ticks until the dash can be used again (0 = ready).
    pub dash_cooldown: u16,
    pub action: Action,
}

impl Fighter {
    /// Fresh fighter at full HP, idle, with every cooldown ready.
    pub fn new(species: SpeciesId, pos: Vec2, facing: Dir8) -> Self {
        let pokemon = crate::data::species(species);
        let max_hp = crate::combat::max_hp(pokemon.base.hp);
        Self {
            species,
            pos,
            facing,
            hp: max_hp,
            max_hp,
            move_cooldowns: [0, 0],
            dash_cooldown: 0,
            action: Action::Idle,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Projectile {
    /// Index into `State::fighters`.
    pub owner: u8,
    /// Radius, power and type are looked up from here, not copied.
    pub move_id: MoveId,
    pub pos: Vec2,
    /// Units per tick.
    pub velocity: Vec2,
    /// Distance left before it expires.
    pub range_left: i32,
}

/// A telegraphed area that lands when `ticks_left` reaches 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zone {
    /// Index into `State::fighters`.
    pub owner: u8,
    pub move_id: MoveId,
    pub center: Vec2,
    pub ticks_left: u16,
}

/// How a match ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Index of the winning fighter.
    Winner(u8),
    Draw,
}

/// Not `Copy`: the `Vec`s own heap memory, so copying needs an explicit
/// `.clone()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub tick: u32,
    pub rng: Rng,
    /// Half-width and half-height. The arena spans `-arena..=arena`, centered
    /// on the origin.
    pub arena: Vec2,
    pub fighters: [Fighter; 2],
    pub projectiles: Vec<Projectile>,
    pub zones: Vec<Zone>,
    /// `None` while the match is running.
    pub outcome: Option<Outcome>,
}

impl State {
    /// Tick 0 of a match (`reset` in the sim API). Fighters spawn mirrored
    /// across the center, facing each other; `species[i]` is fighter `i`.
    pub fn new(seed: u64, species: [SpeciesId; 2]) -> Self {
        Self {
            tick: 0,
            rng: Rng::new(seed),
            arena: ARENA_HALF_EXTENTS,
            fighters: [
                Fighter::new(species[0], Vec2::new(-SPAWN_OFFSET, 0), Dir8::E),
                Fighter::new(species[1], Vec2::new(SPAWN_OFFSET, 0), Dir8::W),
            ],
            projectiles: Vec::new(),
            zones: Vec::new(),
            outcome: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{BLASTOISE, PIKACHU};

    #[test]
    fn default_intent_does_nothing() {
        let intent = Intent::default();
        assert_eq!(intent.movement, None);
        assert!(!intent.dash);
        assert_eq!(intent.move_slot, None);
    }

    fn slice_match() -> State {
        State::new(42, [PIKACHU, BLASTOISE])
    }

    #[test]
    fn fighters_start_at_full_hp() {
        let state = slice_match();
        for f in &state.fighters {
            assert_eq!(f.hp, f.max_hp);
        }
        assert_eq!(state.fighters[0].max_hp, 2200);
        assert_eq!(state.fighters[1].max_hp, 3080);
    }

    #[test]
    fn fighters_start_idle_with_cooldowns_ready() {
        for f in &slice_match().fighters {
            assert_eq!(f.action, Action::Idle);
            assert_eq!(f.move_cooldowns, [0, 0]);
            assert_eq!(f.dash_cooldown, 0);
        }
    }

    #[test]
    fn spawns_are_mirrored() {
        let state = slice_match();
        assert_eq!(state.fighters[1].pos, -state.fighters[0].pos);
        assert_eq!(state.fighters[0].pos, Vec2::new(-SPAWN_OFFSET, 0));
    }

    #[test]
    fn fighters_face_each_other() {
        let state = slice_match();
        assert_eq!(state.fighters[0].facing, Dir8::E);
        assert_eq!(state.fighters[1].facing, Dir8::W);
    }

    #[test]
    fn match_starts_empty() {
        let state = slice_match();
        assert_eq!(state.tick, 0);
        assert!(state.projectiles.is_empty());
        assert!(state.zones.is_empty());
        assert_eq!(state.outcome, None);
    }

    #[test]
    fn same_seed_gives_equal_states() {
        assert_eq!(slice_match(), slice_match());
        assert_ne!(
            State::new(1, [PIKACHU, BLASTOISE]),
            State::new(2, [PIKACHU, BLASTOISE])
        );
    }
}
