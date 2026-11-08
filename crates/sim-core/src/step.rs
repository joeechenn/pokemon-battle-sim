//! The reducer: `step(state, intents) -> events`.
//!
//! Phases run in this fixed order every tick:
//! 1. Read intents and start actions where allowed.
//! 2. Move fighters, apply dashes, clamp to the arena, separate overlaps.
//! 3. Advance windups; release finished moves (spawn projectile, spawn zone,
//!    start dash-attack).
//! 4. Move projectiles and test hits.
//! 5. Count down zones; detonate and test hits.
//! 6. Apply damage and faints.
//! 7. Tick cooldowns down.
//! 8. Check end conditions (faint, timeout).
//!
//! Both fighters move before any hit is tested, so fighter 0 gets no advantage
//! from being processed first.
//!
//! Rust concepts: `&mut` borrowing, iterating by index, `match` on state enums.
