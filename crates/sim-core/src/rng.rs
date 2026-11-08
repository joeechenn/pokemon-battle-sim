//! Deterministic random number generator stored in `State`.
//!
//! Hand-written so the exact sequence is identical on native and WASM builds
//! and across Rust versions. Seeded once by `reset(seed)`.
