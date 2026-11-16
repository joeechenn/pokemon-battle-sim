//! Native-vs-WASM determinism check.
//!
//! Exports the reference match's hash from a WebAssembly build, so
//! `check.mjs` can compare it with the value the native tests pin. Equal
//! values mean the game plays tick-for-tick the same on both platforms.
//!
//! Run from the `sim/` folder:
//!
//! ```text
//! cargo build -p wasm-check --target wasm32-unknown-unknown --release
//! node crates/wasm-check/check.mjs
//! ```

use sim_core::replay::{REFERENCE_MATCH_HASH, reference_match};

/// The reference match's hash, computed by this (WASM) build.
///
/// `extern "C"` gives the function a plain calling convention JavaScript can
/// call; `no_mangle` keeps its name readable in the module's exports. A `u64`
/// arrives in JavaScript as a `BigInt`.
#[unsafe(no_mangle)]
pub extern "C" fn reference_match_hash() -> u64 {
    reference_match().hash()
}

/// The value the native build pinned.
#[unsafe(no_mangle)]
pub extern "C" fn expected_reference_match_hash() -> u64 {
    REFERENCE_MATCH_HASH
}
