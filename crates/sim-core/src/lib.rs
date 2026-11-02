//! Deterministic battle simulation core.
//!
//! Rules (see DESIGN.md §4): fixed timestep, seeded RNG stored in state,
//! fixed-point math, no HashMap iteration, no I/O, entities in arrays
//! referenced by index, events out, presentation never mutates state.

/// Spec version for the observation/action layout. Written into every weights
/// file; loaders refuse a mismatch.
pub const SPEC_VERSION: u32 = 0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_version_is_defined() {
        assert_eq!(SPEC_VERSION, 0);
    }
}
