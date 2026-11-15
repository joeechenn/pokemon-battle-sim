//! The reducer: `step(state, intents) -> events`.
//!
//! Each tick runs these steps in a fixed order:
//! 1. Read intents and start actions where allowed.
//! 2. Move fighters, apply dashes, separate overlaps, clamp to the arena.
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

use crate::combat::move_speed;
use crate::data;
use crate::event::Event;
use crate::math::Vec2;
use crate::state::{Action, Fighter, Intent, State};

// Universal dash: a short movement burst any fighter can use. Starting values,
// to be tuned through play. Distances in units, durations in ticks.

/// How far one dash travels: 3 tiles.
pub const DASH_DISTANCE: i32 = 3000;
/// How many ticks a dash lasts: 0.2 s.
pub const DASH_TICKS: u16 = 6;
/// Units moved per dashing tick. The one place distance meets duration, so the
/// `u16` duration is widened to `i32` here instead of at every use.
pub const DASH_SPEED: i32 = DASH_DISTANCE / DASH_TICKS as i32;
/// Ticks from starting a dash until the next one is allowed: 2 s.
pub const DASH_COOLDOWN: u16 = 60;
/// Ticks spent in `Recovering` after a dash ends, before acting again.
pub const DASH_RECOVERY: u16 = 3;

/// Turns intents into new actions.
///
/// Only an `Idle` fighter reads its intent; a fighter that is dashing,
/// casting or recovering ignores input until it is idle again. This only
/// starts actions; what they do each tick is handled by the steps after it.
fn start_actions(fighters: &mut [Fighter; 2], intents: &[Intent; 2], events: &mut Vec<Event>) {
    for (i, f) in fighters.iter_mut().enumerate() {
        // Only an idle fighter can start something new.
        if f.action != Action::Idle {
            continue;
        }
        let intent = intents[i];
        // Dash is checked first, so it wins over a move in the same intent.
        if intent.dash && f.dash_cooldown == 0 {
            // Dash where the input points, or straight ahead with no input.
            let direction = intent.movement.unwrap_or(f.facing);
            // Direction and speed are fixed here; the dash can't be steered.
            let velocity = direction.to_vec().with_length(DASH_SPEED);
            f.action = Action::Dashing {
                velocity,
                ticks_left: DASH_TICKS,
            };
            f.dash_cooldown = DASH_COOLDOWN;
            f.facing = direction;
            events.push(Event::Dashed {
                fighter: i as u8,
                direction,
            });
        }
        // TODO: start a move here (`else if let Some(slot) = intent.move_slot`).
    }
}

/// Advances each fighter's movement by one tick.
///
/// Idle fighters walk if their intent has a direction. Dashes move by their
/// stored velocity. Action timers (`ticks_left`) count down here; ability
/// cooldowns count down separately in `tick_cooldowns`.
fn move_fighters(fighters: &mut [Fighter; 2], intents: &[Intent; 2]) {
    for (i, f) in fighters.iter_mut().enumerate() {
        let intent = intents[i];
        // Match on a copy of the action (`Action` is `Copy`), so each arm can
        // overwrite `f.action` without a borrow conflict.
        match f.action {
            Action::Idle => {
                if let Some(dir) = intent.movement {
                    let spe = data::species(f.species).base.spe;
                    let speed = move_speed(spe);
                    f.facing = dir;
                    // Rescaling the direction keeps diagonals as fast as
                    // straight lines, not sqrt(2) faster.
                    f.pos += dir.to_vec().with_length(speed);
                }
            }
            Action::Dashing {
                velocity,
                ticks_left,
            } => {
                f.pos += velocity;
                // `ticks_left` includes this tick, so 1 means this was the last
                // one. `> 1` also ends safely if a 0 ever slips in.
                f.action = if ticks_left > 1 {
                    Action::Dashing {
                        velocity,
                        ticks_left: ticks_left - 1,
                    }
                } else {
                    Action::Recovering {
                        ticks_left: DASH_RECOVERY,
                    }
                };
            }
            // Casting fighters stand still. The windup timer will count down
            // alongside move release, not here (TODO).
            Action::Casting { .. } => {}
            // Standing still; only the timer runs.
            Action::Recovering { ticks_left } => {
                f.action = if ticks_left > 1 {
                    Action::Recovering {
                        ticks_left: ticks_left - 1,
                    }
                } else {
                    Action::Idle
                };
            }
            // TODO: dash-attack movement and contact hits.
            Action::Lunging { .. } => {}
        }
    }
}

/// Pushes overlapping fighters apart, half each.
///
/// Fighters are circles, so they overlap when their centers are closer than
/// the sum of their radii. Splitting the push equally keeps either side from
/// being favored.
fn separate_fighters(fighters: &mut [Fighter; 2]) {
    // Read first: copy out what's needed so no borrow of `fighters` is held
    // while computing. Writes happen at the end, one short borrow each.
    let p0 = fighters[0].pos;
    let p1 = fighters[1].pos;

    let r0 = data::species(fighters[0].species).radius;
    let r1 = data::species(fighters[1].species).radius;

    // `d` points from fighter 0 to fighter 1. `min` is the closest the centers
    // can be with the bodies just touching.
    let d = p1 - p0;
    let min = r0 + r1;

    let dist = d.length();

    if dist >= min {
        return;
    }

    let overlap = min - dist;

    // Exactly on top of each other: `d` has no direction, so use a fixed one
    // (fighter 0 left, fighter 1 right). Fixed, not random, to stay
    // deterministic.
    let direction = if dist == 0 { Vec2::new(1, 0) } else { d };

    // `overlap / 2` rounds down, so an odd overlap leaves them 1 unit short;
    // the next tick corrects it.
    let push = direction.with_length(overlap / 2);
    fighters[0].pos -= push;
    fighters[1].pos += push;
}

/// Keeps every fighter's whole body inside the arena.
///
/// Runs after `separate_fighters`, so no one ends a tick outside the walls
/// even if separation pushed them toward one. `arena` is the half-extents.
fn clamp_to_arena(arena: Vec2, fighters: &mut [Fighter; 2]) {
    for f in fighters.iter_mut() {
        // `pos` is the center, so it stops one radius short of each wall.
        let r = data::species(f.species).radius;
        let limit_x = arena.x - r;
        let limit_y = arena.y - r;
        f.pos.x = f.pos.x.clamp(-limit_x, limit_x);
        f.pos.y = f.pos.y.clamp(-limit_y, limit_y);
    }
}

/// Counts every ability cooldown down by one, stopping at 0.
///
/// Runs last, so a cooldown set this tick is already one lower when the tick
/// ends: a 60-tick cooldown allows reuse exactly 60 ticks later.
fn tick_cooldowns(fighters: &mut [Fighter; 2]) {
    for f in fighters.iter_mut() {
        // Saturating: a ready ability sits at 0, and `0 - 1` on a `u16` would
        // panic with overflow checks on.
        f.dash_cooldown = f.dash_cooldown.saturating_sub(1);
        for c in f.move_cooldowns.iter_mut() {
            // `c` is a `&mut u16`; `*c =` writes into the array slot.
            *c = c.saturating_sub(1);
        }
    }
}

/// Advance the match by one tick. `intents[i]` is fighter `i`'s input.
///
/// Mutates `state` in place and returns what happened this tick for the
/// viewer and client. A finished match no longer changes.
pub fn step(state: &mut State, intents: [Intent; 2]) -> Vec<Event> {
    if state.outcome.is_some() {
        return Vec::new();
    }
    let mut events = Vec::new();

    // Order matters; see the module docs.
    start_actions(&mut state.fighters, &intents, &mut events);
    // Move, then separate, then clamp, so walls always win.
    move_fighters(&mut state.fighters, &intents);
    separate_fighters(&mut state.fighters);
    // `state.arena` is copied while `state.fighters` is borrowed: different
    // fields, so both are allowed at once.
    clamp_to_arena(state.arena, &mut state.fighters);
    // TODO: moves, projectiles, zones, damage and end-of-match checks.
    tick_cooldowns(&mut state.fighters);

    state.tick += 1;
    events
}
