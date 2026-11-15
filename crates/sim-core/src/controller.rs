//! Controllers produce one `Intent` per fighter per tick.
//!
//! TODO: `trait Controller { fn intent(&mut self, state: &State, me: usize)
//! -> Intent; }`, plus a scripted controller for tests. Keyboard and AI
//! controllers implement the same trait.
