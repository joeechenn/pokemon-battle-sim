//! Stable per-tick state hash for determinism tests.
//!
//! Rust's built-in `DefaultHasher` is not guaranteed stable across compiler
//! versions, so this uses its own hash over every state field in a fixed order.
