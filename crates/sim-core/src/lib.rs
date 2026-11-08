//! Deterministic battle simulation core.
//!
//! The sim is a reducer: `step(state, intents) -> (new state, events)`, run at
//! a fixed 30 ticks per second.
//!
//! Rules: fixed timestep, seeded RNG stored in state,
//! integer math, no HashMap iteration, no I/O, entities in arrays
//! referenced by index, events out, presentation never mutates state.

pub mod combat;
pub mod controller;
pub mod data;
pub mod event;
pub mod hash;
pub mod math;
pub mod rng;
pub mod state;
pub mod step;

/// Spec version for the observation/action layout. Written into every weights
/// file; loaders refuse a mismatch.
pub const SPEC_VERSION: u32 = 0;

/// Simulation ticks per second. All durations in data are counted in ticks.
pub const TICKS_PER_SECOND: u32 = 30;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_version_is_defined() {
        assert_eq!(SPEC_VERSION, 0);
    }
}
