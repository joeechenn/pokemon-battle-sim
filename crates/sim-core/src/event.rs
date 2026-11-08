//! Events emitted by `step` for the viewer and client to present.
//!
//! Output only: nothing in the sim reads events back.
//!
//! To add: an `Event` enum with variants such as `MoveStarted`,
//! `ProjectileSpawned`, `TelegraphStarted`, `Hit { target, damage,
//! effectiveness }`, `Dashed`, `Fainted`, `MatchEnded`.
