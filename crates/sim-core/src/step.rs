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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{BLASTOISE, PIKACHU};
    use crate::math::Dir8;
    use crate::state::Outcome;

    /// Fighter 0 is Pikachu at (-6000, 0) facing east; fighter 1 is Blastoise.
    fn new_match() -> State {
        State::new(42, [PIKACHU, BLASTOISE])
    }

    /// `intent` for fighter 0; fighter 1 does nothing.
    fn only_fighter_0(intent: Intent) -> [Intent; 2] {
        [intent, Intent::default()]
    }

    fn walk(dir: Dir8) -> Intent {
        Intent {
            movement: Some(dir),
            ..Intent::default()
        }
    }

    fn dash() -> Intent {
        Intent {
            dash: true,
            ..Intent::default()
        }
    }

    #[test]
    fn walking_moves_by_walk_speed() {
        let mut state = new_match();
        let start = state.fighters[0].pos;
        step(&mut state, only_fighter_0(walk(Dir8::E)));
        // Pikachu's walk speed is 157 units per tick.
        assert_eq!(state.fighters[0].pos, start + Vec2::new(157, 0));
    }

    #[test]
    fn walking_turns_to_face_the_direction() {
        let mut state = new_match();
        step(&mut state, only_fighter_0(walk(Dir8::N)));
        assert_eq!(state.fighters[0].facing, Dir8::N);
    }

    #[test]
    fn no_input_changes_nothing_but_the_tick() {
        let mut state = new_match();
        let before = state.clone();
        let events = step(&mut state, [Intent::default(); 2]);
        assert!(events.is_empty());
        assert_eq!(state.fighters, before.fighters);
        assert_eq!(state.tick, before.tick + 1);
    }

    #[test]
    fn dash_goes_forward_then_recovers_then_idles() {
        let mut state = new_match();
        let start = state.fighters[0].pos;

        // No movement input, so the dash goes the way Pikachu faces (east).
        let events = step(&mut state, only_fighter_0(dash()));
        assert_eq!(
            events,
            vec![Event::Dashed {
                fighter: 0,
                direction: Dir8::E,
            }]
        );

        // Five more ticks finish the dash. Holding dash doesn't restart it.
        for _ in 0..5 {
            let events = step(&mut state, only_fighter_0(dash()));
            assert!(events.is_empty());
        }
        assert_eq!(state.fighters[0].pos, start + Vec2::new(DASH_DISTANCE, 0));
        assert_eq!(
            state.fighters[0].action,
            Action::Recovering {
                ticks_left: DASH_RECOVERY,
            }
        );

        // Recovery lasts DASH_RECOVERY ticks, without moving, then idle.
        for _ in 0..DASH_RECOVERY {
            step(&mut state, only_fighter_0(walk(Dir8::E)));
        }
        assert_eq!(state.fighters[0].pos, start + Vec2::new(DASH_DISTANCE, 0));
        assert_eq!(state.fighters[0].action, Action::Idle);
    }

    #[test]
    fn dash_follows_movement_input() {
        let mut state = new_match();
        let intent = Intent {
            movement: Some(Dir8::S),
            dash: true,
            ..Intent::default()
        };
        step(&mut state, only_fighter_0(intent));
        assert_eq!(state.fighters[0].facing, Dir8::S);
        assert_eq!(
            state.fighters[0].pos,
            Vec2::new(-6_000, DASH_SPEED),
            "y points down, so south is +y"
        );
    }

    #[test]
    fn dash_is_ready_again_exactly_one_cooldown_later() {
        let mut state = new_match();
        let mut dash_ticks = Vec::new();
        // Hold dash for a little over one cooldown.
        for _ in 0..=DASH_COOLDOWN {
            let tick = state.tick;
            let events = step(&mut state, only_fighter_0(dash()));
            if !events.is_empty() {
                dash_ticks.push(tick);
            }
        }
        assert_eq!(dash_ticks, vec![0, u32::from(DASH_COOLDOWN)]);
    }

    #[test]
    fn walls_stop_the_whole_body() {
        let mut state = new_match();
        for _ in 0..200 {
            step(&mut state, only_fighter_0(walk(Dir8::W)));
        }
        let radius = data::species(PIKACHU).radius;
        assert_eq!(state.fighters[0].pos.x, -state.arena.x + radius);
        assert_eq!(state.fighters[0].pos.y, 0);
    }

    #[test]
    fn overlapping_fighters_are_pushed_apart() {
        let mut state = new_match();
        state.fighters[0].pos = Vec2::new(0, 0);
        state.fighters[1].pos = Vec2::new(100, 0);
        step(&mut state, [Intent::default(); 2]);

        let min = data::species(PIKACHU).radius + data::species(BLASTOISE).radius;
        let dist = (state.fighters[1].pos - state.fighters[0].pos).length();
        // Rounding the half-push down can leave them 1 unit short.
        assert!(dist >= min - 1, "still overlapping: {dist} < {min}");
        // Overlap was 800, so each moved 400: neither side is favored.
        assert_eq!(state.fighters[0].pos, Vec2::new(-400, 0));
        assert_eq!(state.fighters[1].pos, Vec2::new(500, 0));
    }

    #[test]
    fn fighters_on_the_same_spot_still_separate() {
        let mut state = new_match();
        state.fighters[0].pos = Vec2::ZERO;
        state.fighters[1].pos = Vec2::ZERO;
        step(&mut state, [Intent::default(); 2]);

        assert!(state.fighters[0].pos.x < 0, "fighter 0 goes left");
        assert!(state.fighters[1].pos.x > 0, "fighter 1 goes right");
    }

    #[test]
    fn finished_match_does_not_change() {
        let mut state = new_match();
        state.outcome = Some(Outcome::Draw);
        let before = state.clone();
        let events = step(&mut state, only_fighter_0(dash()));
        assert!(events.is_empty());
        assert_eq!(state, before);
    }

    #[test]
    fn same_inputs_give_same_states() {
        let mut a = new_match();
        let mut b = new_match();
        let script = [walk(Dir8::NE), dash(), walk(Dir8::W), Intent::default()];
        for intent in script.iter().cycle().take(120) {
            step(&mut a, [*intent, walk(Dir8::SW)]);
            step(&mut b, [*intent, walk(Dir8::SW)]);
        }
        assert_eq!(a, b);
    }
}
