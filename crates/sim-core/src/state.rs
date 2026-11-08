//! Everything that changes during a match, plus per-tick inputs.
//!
//! To add:
//! - `State`: tick, RNG, arena bounds, two fighters, projectiles, telegraph
//!   zones, outcome.
//! - `Fighter`: species, position, facing, HP, move and dash cooldowns, and an
//!   action state enum: `Idle | Dashing | Casting | Recovering`, each carrying
//!   its own data (ticks left, direction, move slot).
//! - `Intent`: movement direction (or none), dash, move slot (or none). Mirrors
//!   the brain's action space in DESIGN.md §5.
//!
//! Entities live in `Vec`s and arrays and refer to each other by index, never
//! by reference. That keeps the borrow checker happy and the state hashable.
//!
//! Rust concepts: `Vec`, arrays, `Option`, enums with data.
