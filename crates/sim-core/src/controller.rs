//! Controllers produce one `Intent` per fighter per tick.
//!
//! To add: `trait Controller { fn intent(&mut self, state: &State, me: usize)
//! -> Intent; }`, plus a scripted controller for tests. The keyboard
//! controller lives in the viewer; the utility AI and the brain come later.
//!
//! Rust concepts: traits (like TypeScript interfaces), `&mut self`.
