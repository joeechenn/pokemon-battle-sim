//! Events emitted by `step` for the viewer and client to present.
//!
//! Output only: nothing in the sim reads events back.
//!
//! An event marks a discrete moment presentation reacts to (a callout, sound,
//! effect, damage number) that it can't reliably work out from `State` alone.
//! Continuous things like movement are not events; read them from the state.
//!
//! Fighters are referred to by index into `State::fighters`.

use crate::data::MoveId;
use crate::math::{Dir8, Vec2};
use crate::state::Outcome;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Windup began. Shows the move-name callout.
    MoveStarted { fighter: u8, move_id: MoveId },
    /// A projectile was released at `pos`.
    ProjectileSpawned {
        owner: u8,
        move_id: MoveId,
        pos: Vec2,
    },
    /// A projectile ran out of range without hitting anything.
    ProjectileExpired { move_id: MoveId, pos: Vec2 },
    /// A telegraphed zone appeared; it lands in `delay` ticks.
    TelegraphStarted {
        owner: u8,
        move_id: MoveId,
        center: Vec2,
        delay: u16,
    },
    /// A zone detonated. Sent whether or not it hit; hits follow as `Hit`.
    ZoneLanded {
        owner: u8,
        move_id: MoveId,
        center: Vec2,
    },
    /// The universal movement dash started.
    Dashed { fighter: u8, direction: Dir8 },
    /// A move connected. One variant for every archetype.
    Hit {
        attacker: u8,
        target: u8,
        move_id: MoveId,
        /// HP removed, after the type multiplier.
        damage: i32,
        /// Type multiplier x4, from `Type::effectiveness` (4 = neutral).
        effectiveness: u8,
    },
    /// HP reached 0.
    Fainted { fighter: u8 },
    /// The match is decided. Sent once, on the final tick.
    MatchEnded { outcome: Outcome },
}
