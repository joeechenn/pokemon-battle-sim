//! Static game data: types, species, moves.
//!
//! Read-only during a match. Hard-coded constants for the slice; later this
//! becomes the data schema the sim repo owns and loads from files.
//!
//! To add:
//! - `Type` enum and the type chart lookup (effectiveness multiplier).
//! - `Species`: name, types, base stats.
//! - `Move`: type, physical/special, power, windup, cooldown, recovery, and an
//!   archetype with its parameters (projectile, telegraphed zone, dash-attack).
//! - Slice data: Pikachu (Thunderbolt, Thunder) and Blastoise (Water Pulse,
//!   Aqua Jet).
//!
//! Rust concepts: enums, `match`, `const` and `static` data.
