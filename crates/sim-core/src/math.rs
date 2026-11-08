//! Integer math and units.
//!
//! Units: 1 tile = 1000 units, stored as `i32`. Speeds are units per tick.
//! Use `i64` for intermediate products so multiplication can't overflow.
//!
//! To add:
//! - `Vec2 { x: i32, y: i32 }` with add, subtract, and scale.
//! - Integer square root, length, distance, and normalize-to-length.
//! - `Dir8`: the 8 movement directions, each mapped to a unit vector.
//! - Unit tests for each helper.
//!
//! Rust concepts: structs, `impl` blocks, operator traits (`Add`, `Sub`),
//! `#[derive(...)]`, `#[cfg(test)]` test modules.
