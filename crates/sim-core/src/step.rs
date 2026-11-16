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

use crate::combat::{damage, move_speed};
use crate::data;
use crate::data::Archetype;
use crate::event::Event;
use crate::math::Vec2;
use crate::state::{Action, Fighter, Intent, Outcome, Projectile, State, Zone};
use std::cmp::Ordering;

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

/// Match length: 60 seconds. When it runs out, the higher HP percentage wins.
/// Starting value, to be tuned through play.
pub const MATCH_TICKS: u32 = 60 * crate::TICKS_PER_SECOND;

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
        } else if let Some(slot) = intent.move_slot {
            let species = data::species(f.species);
            let s = usize::from(slot);
            // Bad slots are ignored, never a panic. The range check comes
            // first so the cooldown lookup can't index out of bounds.
            if s < species.moves.len() && f.move_cooldowns[s] == 0 {
                let move_id = species.moves[s];
                let mv = data::move_data(move_id);
                f.action = Action::Casting {
                    slot,
                    ticks_left: mv.windup,
                };
                events.push(Event::MoveStarted {
                    fighter: i as u8,
                    move_id,
                });
            }
        }
    }
}

/// Advances each fighter's movement by one tick.
///
/// Idle fighters walk if their intent has a direction. Dashes move by their
/// stored velocity. Dash and recovery timers count down here; windups count
/// down in `advance_casts`, and ability cooldowns in `tick_cooldowns`.
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
            // Casting fighters stand still; `advance_casts` runs the windup.
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
            // A lunge that runs its full length missed; contact hits are
            // checked separately, after separation and the wall clamp.
            Action::Lunging {
                slot,
                velocity,
                ticks_left,
            } => {
                f.pos += velocity;
                let species = data::species(f.species);
                let mv = data::move_data(species.moves[usize::from(slot)]);
                f.action = if ticks_left > 1 {
                    Action::Lunging {
                        slot,
                        velocity,
                        ticks_left: ticks_left - 1,
                    }
                } else {
                    Action::Recovering {
                        ticks_left: mv.recovery,
                    }
                };
            }
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

/// Counts windups down and releases moves whose windup has finished.
///
/// Moves auto-aim at the opponent's position at the moment of release, so
/// moving during the windup is how a target dodges. Runs after movement, so
/// the aim uses where everyone ended up this tick. The cooldown starts at
/// release.
fn advance_casts(state: &mut State, events: &mut Vec<Event>) {
    // Copied up front: the loop borrows `fighters` mutably, so it can't also
    // read the opponent's position from it.
    let positions = [state.fighters[0].pos, state.fighters[1].pos];

    for (i, f) in state.fighters.iter_mut().enumerate() {
        let Action::Casting { slot, ticks_left } = f.action else {
            continue;
        };
        if ticks_left > 1 {
            f.action = Action::Casting {
                slot,
                ticks_left: ticks_left - 1,
            };
            continue;
        }
        let species = data::species(f.species);
        let s = usize::from(slot);
        let move_id = species.moves[s];
        let mv = data::move_data(move_id);
        // `1 - i` is the opponent's index. On the same spot there's no
        // direction to aim, so fire straight ahead.
        let aim = positions[1 - i] - f.pos;
        let aim = if aim == Vec2::ZERO {
            f.facing.to_vec()
        } else {
            aim
        };
        f.move_cooldowns[s] = mv.cooldown;
        match mv.archetype {
            Archetype::Projectile { speed, range, .. } => {
                state.projectiles.push(Projectile {
                    owner: i as u8,
                    move_id,
                    pos: f.pos,
                    velocity: aim.with_length(speed),
                    range_left: range,
                });
                f.action = Action::Recovering {
                    ticks_left: mv.recovery,
                };
                events.push(Event::ProjectileSpawned {
                    owner: i as u8,
                    move_id,
                    pos: f.pos,
                });
            }
            Archetype::Zone {
                delay, cast_range, ..
            } => {
                // On the target if it's within reach; otherwise as far as the
                // move reaches, in the target's direction.
                let center = if aim.length() > cast_range {
                    f.pos + aim.with_length(cast_range)
                } else {
                    positions[1 - i]
                };
                state.zones.push(Zone {
                    owner: i as u8,
                    move_id,
                    center,
                    ticks_left: delay,
                });
                f.action = Action::Recovering {
                    ticks_left: mv.recovery,
                };
                events.push(Event::TelegraphStarted {
                    owner: i as u8,
                    move_id,
                    center,
                    delay,
                });
            }
            // Nothing spawns: the fighter itself becomes the attack. Its
            // recovery starts when the lunge ends.
            Archetype::DashAttack {
                distance, duration, ..
            } => {
                f.action = Action::Lunging {
                    slot,
                    velocity: aim.with_length(distance / duration as i32),
                    ticks_left: duration,
                };
            }
        }
    }
}

/// Moves projectiles, applies hits, and removes spent ones.
///
/// A projectile hits the opponent (never its owner) when their circles
/// overlap, and expires at the end of its range or when its edge touches a
/// wall. A hit is checked before expiry, so a hit on the last tick of range
/// still counts. HP is lowered here; faints are checked after every hit this
/// tick has been applied, so simultaneous knockouts are treated fairly.
fn advance_projectiles(state: &mut State, events: &mut Vec<Event>) {
    state.projectiles.retain_mut(|p| {
        p.pos += p.velocity;
        p.range_left -= p.velocity.length();
        let mv = data::move_data(p.move_id);
        let Archetype::Projectile { radius, .. } = mv.archetype else {
            unreachable!("projectile spawned from a non-projectile move");
        };
        // Look each species up once. `attacker` points into the static
        // species table, not into `state`, so holding it doesn't block the
        // mutable borrow of the target below.
        let attacker = data::species(state.fighters[usize::from(p.owner)].species);
        let target = &mut state.fighters[usize::from(1 - p.owner)];
        let target_species = data::species(target.species);

        let dist = (target.pos - p.pos).length();

        let outside =
            p.pos.x.abs() + radius > state.arena.x || p.pos.y.abs() + radius > state.arena.y;
        if dist < radius + target_species.radius {
            let dmg = damage(attacker, target_species, mv);
            target.hp = (target.hp - dmg).max(0);
            events.push(Event::Hit {
                attacker: p.owner,
                target: 1 - p.owner,
                move_id: p.move_id,
                damage: dmg,
                effectiveness: mv.move_type.effectiveness(target_species.types),
            });
            return false;
        }
        if p.range_left <= 0 || outside {
            events.push(Event::ProjectileExpired {
                move_id: p.move_id,
                pos: p.pos,
            });
            return false;
        }
        true
    });
}

/// Counts telegraphed zones down and lands the ones whose time is up.
///
/// A zone lands on the tick its `ticks_left` reaches 1, so it lands exactly
/// `delay` ticks after it was placed. Landing always emits `ZoneLanded`; if the
/// opponent's body overlaps the circle (the caster is never hit), a `Hit`
/// follows. A landed zone is removed whether or not it hit.
fn advance_zones(state: &mut State, events: &mut Vec<Event>) {
    state.zones.retain_mut(|z| {
        if z.ticks_left > 1 {
            z.ticks_left -= 1;
            return true;
        }

        events.push(Event::ZoneLanded {
            owner: z.owner,
            move_id: z.move_id,
            center: z.center,
        });

        let mv = data::move_data(z.move_id);
        let Archetype::Zone { radius, .. } = mv.archetype else {
            unreachable!("zone spawned from a non-zone move");
        };
        // Same lookups as projectiles: `attacker` points into the static
        // species table, so it doesn't conflict with borrowing the target.
        let attacker = data::species(state.fighters[usize::from(z.owner)].species);
        let target = &mut state.fighters[usize::from(1 - z.owner)];
        let target_species = data::species(target.species);

        let dist = (target.pos - z.center).length();
        if dist < radius + target_species.radius {
            let dmg = damage(attacker, target_species, mv);
            target.hp = (target.hp - dmg).max(0);
            events.push(Event::Hit {
                attacker: z.owner,
                target: 1 - z.owner,
                move_id: z.move_id,
                damage: dmg,
                effectiveness: mv.move_type.effectiveness(target_species.types),
            });
        }
        // Landed: removed whether or not it hit, so it can't land again.
        false
    });
}

/// Checks lunging fighters for contact with the opponent.
///
/// The hitbox is the move's own radius around the lunging fighter, slightly
/// larger than its body, so it still connects after `separate_fighters` has
/// pushed the two bodies apart. A hit ends the lunge immediately and starts the
/// move's recovery, so a lunge hits at most once.
fn lunge_hits(state: &mut State, events: &mut Vec<Event>) {
    // The loop borrows `fighters` mutably, so the opponent is read from these
    // copies and its damage is applied after the loop.
    let positions = [state.fighters[0].pos, state.fighters[1].pos];
    let species_ids = [state.fighters[0].species, state.fighters[1].species];
    let mut damage_taken = [0; 2];

    for (i, f) in state.fighters.iter_mut().enumerate() {
        let Action::Lunging { slot, .. } = f.action else {
            continue;
        };
        let species = data::species(f.species);
        let move_id = species.moves[usize::from(slot)];
        let mv = data::move_data(move_id);
        let Archetype::DashAttack { radius, .. } = mv.archetype else {
            unreachable!("lunge from a non-dash-attack move");
        };

        let t = 1 - i;
        let target_species = data::species(species_ids[t]);
        let dist = (positions[t] - f.pos).length();
        if dist < radius + target_species.radius {
            let dmg = damage(species, target_species, mv);
            damage_taken[t] += dmg;
            events.push(Event::Hit {
                attacker: i as u8,
                target: t as u8,
                move_id,
                damage: dmg,
                effectiveness: mv.move_type.effectiveness(target_species.types),
            });
            f.action = Action::Recovering {
                ticks_left: mv.recovery,
            };
        }
    }

    for (f, dmg) in state.fighters.iter_mut().zip(damage_taken) {
        f.hp = (f.hp - dmg).max(0);
    }
}

/// Decides whether the match is over, and how.
///
/// Runs last in `step`, after every hit this tick, so knockouts in the same
/// tick count as simultaneous. Faints decide first: one fighter down loses,
/// both down is a draw. Otherwise, once `MATCH_TICKS` have passed, the higher
/// HP percentage wins. When the match ends, `Fainted` events come first, then
/// `MatchEnded`, and `state.outcome` is set so later steps do nothing.
fn check_end(state: &mut State, events: &mut Vec<Event>) {
    for (i, f) in state.fighters.iter().enumerate() {
        if f.hp == 0 {
            events.push(Event::Fainted { fighter: i as u8 });
        }
    }

    let down = [state.fighters[0].hp == 0, state.fighters[1].hp == 0];
    let outcome = match down {
        [true, true] => Some(Outcome::Draw),
        [true, false] => Some(Outcome::Winner(1)),
        [false, true] => Some(Outcome::Winner(0)),
        // Match guard: this arm only applies once time is up.
        [false, false] if state.tick >= MATCH_TICKS => Some(timeout_result(&state.fighters)),
        [false, false] => None,
    };

    if let Some(outcome) = outcome {
        state.outcome = Some(outcome);
        events.push(Event::MatchEnded { outcome });
    }
}

/// Who wins when time runs out: the higher HP percentage, or a draw if equal.
///
/// Compares `hp0 / max0` with `hp1 / max1` by cross-multiplying
/// (`hp0 * max1` vs `hp1 * max0`), which is exact with integers; dividing
/// first would round both sides down and could hide the difference.
fn timeout_result(fighters: &[Fighter; 2]) -> Outcome {
    let [a, b] = fighters;
    let share_0 = i64::from(a.hp) * i64::from(b.max_hp);
    let share_1 = i64::from(b.hp) * i64::from(a.max_hp);
    match share_0.cmp(&share_1) {
        Ordering::Greater => Outcome::Winner(0),
        Ordering::Less => Outcome::Winner(1),
        Ordering::Equal => Outcome::Draw,
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
    lunge_hits(state, &mut events);
    advance_casts(state, &mut events);
    advance_projectiles(state, &mut events);
    advance_zones(state, &mut events);
    tick_cooldowns(&mut state.fighters);
    state.tick += 1;
    // Last, so it sees every hit this tick and the updated tick count.
    check_end(state, &mut events);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{
        AQUA_JET, BLASTOISE, MoveId, PIKACHU, THUNDER, THUNDERBOLT, WATER_PULSE, move_data,
    };
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

    fn use_move(slot: u8) -> Intent {
        Intent {
            move_slot: Some(slot),
            ..Intent::default()
        }
    }

    /// Steps `n` times with the same intents; returns the last tick's events.
    fn run(state: &mut State, intents: [Intent; 2], n: u16) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..n {
            events = step(state, intents);
        }
        events
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

    #[test]
    fn using_a_move_starts_its_windup() {
        let mut state = new_match();
        let events = step(&mut state, only_fighter_0(use_move(0)));
        assert_eq!(
            events,
            vec![Event::MoveStarted {
                fighter: 0,
                move_id: THUNDERBOLT,
            }]
        );
        // The windup already counted down once on the tick it started.
        assert_eq!(
            state.fighters[0].action,
            Action::Casting {
                slot: 0,
                ticks_left: move_data(THUNDERBOLT).windup - 1,
            }
        );
    }

    #[test]
    fn move_on_cooldown_is_ignored() {
        let mut state = new_match();
        state.fighters[0].move_cooldowns[0] = 10;
        let events = step(&mut state, only_fighter_0(use_move(0)));
        assert!(events.is_empty());
        assert_eq!(state.fighters[0].action, Action::Idle);
    }

    #[test]
    fn slot_out_of_range_is_ignored() {
        let mut state = new_match();
        let events = step(&mut state, only_fighter_0(use_move(5)));
        assert!(events.is_empty());
        assert_eq!(state.fighters[0].action, Action::Idle);
    }

    #[test]
    fn dash_wins_over_a_move_in_the_same_tick() {
        let mut state = new_match();
        let intent = Intent {
            dash: true,
            move_slot: Some(0),
            ..Intent::default()
        };
        let events = step(&mut state, only_fighter_0(intent));
        assert!(matches!(events[..], [Event::Dashed { fighter: 0, .. }]));
        assert!(matches!(state.fighters[0].action, Action::Dashing { .. }));
    }

    #[test]
    fn casting_fighter_stands_still() {
        let mut state = new_match();
        let start = state.fighters[0].pos;
        step(&mut state, only_fighter_0(use_move(0)));
        run(&mut state, only_fighter_0(walk(Dir8::N)), 3);
        assert!(matches!(state.fighters[0].action, Action::Casting { .. }));
        assert_eq!(state.fighters[0].pos, start);
    }

    #[test]
    fn thunderbolt_releases_a_projectile_aimed_at_the_target() {
        let mut state = new_match();
        let mv = move_data(THUNDERBOLT);

        // Press on the first tick; the rest of the windup needs no input.
        step(&mut state, only_fighter_0(use_move(0)));
        run(&mut state, [Intent::default(); 2], mv.windup - 2);
        assert!(state.projectiles.is_empty(), "released too early");

        let events = run(&mut state, [Intent::default(); 2], 1);
        assert_eq!(
            state.projectiles,
            vec![Projectile {
                owner: 0,
                move_id: THUNDERBOLT,
                // Spawned at Pikachu (-6000, 0), then already moved one
                // tick, since projectiles advance on the tick they're released.
                pos: Vec2::new(-5_500, 0),
                // Blastoise is due east; 500 is Thunderbolt's speed.
                velocity: Vec2::new(500, 0),
                range_left: 9_000 - 500,
            }]
        );
        assert!(matches!(
            events[..],
            [Event::ProjectileSpawned { owner: 0, .. }]
        ));
        assert_eq!(
            state.fighters[0].action,
            Action::Recovering {
                ticks_left: mv.recovery,
            }
        );
        // Set at release, then counted down once at the end of the same tick.
        assert_eq!(state.fighters[0].move_cooldowns[0], mv.cooldown - 1);
    }

    #[test]
    fn aim_uses_the_target_position_at_release() {
        let mut state = new_match();
        step(&mut state, only_fighter_0(use_move(0)));
        // The target moves during the windup: directly south of Pikachu.
        state.fighters[1].pos = Vec2::new(-6_000, 5_000);
        run(
            &mut state,
            [Intent::default(); 2],
            move_data(THUNDERBOLT).windup,
        );
        assert_eq!(state.projectiles[0].velocity, Vec2::new(0, 500));
    }

    #[test]
    fn thunder_out_of_reach_lands_at_max_range() {
        let mut state = new_match();
        step(&mut state, only_fighter_0(use_move(1)));
        let events = run(
            &mut state,
            [Intent::default(); 2],
            move_data(THUNDER).windup - 1,
        );

        // Blastoise is 12,000 away but Thunder reaches 8,000.
        assert_eq!(state.zones.len(), 1);
        assert_eq!(state.zones[0].center, Vec2::new(2_000, 0));
        assert!(matches!(
            events[..],
            [Event::TelegraphStarted {
                owner: 0,
                delay: 24,
                ..
            }]
        ));
    }

    #[test]
    fn thunder_in_reach_lands_on_the_target() {
        let mut state = new_match();
        state.fighters[1].pos = Vec2::ZERO;
        step(&mut state, only_fighter_0(use_move(1)));
        run(
            &mut state,
            [Intent::default(); 2],
            move_data(THUNDER).windup - 1,
        );
        assert_eq!(state.zones[0].center, Vec2::ZERO);
    }

    #[test]
    fn aqua_jet_starts_a_lunge_toward_the_target() {
        let mut state = new_match();
        let mv = move_data(AQUA_JET);
        // Blastoise is fighter 1, and Aqua Jet is its second move.
        step(&mut state, [Intent::default(), use_move(1)]);
        run(&mut state, [Intent::default(); 2], mv.windup - 1);

        assert_eq!(
            state.fighters[1].action,
            Action::Lunging {
                slot: 1,
                // Pikachu is due west; 4000 units over 8 ticks.
                velocity: Vec2::new(-500, 0),
                ticks_left: 8
            }
        );
        assert!(state.projectiles.is_empty());
        assert!(state.zones.is_empty());
        assert_eq!(state.fighters[1].move_cooldowns[1], mv.cooldown - 1);
    }

    /// A projectile placed directly in the arena, with 9000 range left.
    fn shot(owner: u8, move_id: MoveId, pos: Vec2, velocity: Vec2) -> Projectile {
        Projectile {
            owner,
            move_id,
            pos,
            velocity,
            range_left: 9_000,
        }
    }

    #[test]
    fn projectile_moves_and_spends_range() {
        let mut state = new_match();
        // Above both fighters, so it can't hit anything.
        let start = Vec2::new(0, -5_000);
        state
            .projectiles
            .push(shot(0, THUNDERBOLT, start, Vec2::new(500, 0)));
        let events = step(&mut state, [Intent::default(); 2]);

        assert!(events.is_empty());
        assert_eq!(state.projectiles[0].pos, start + Vec2::new(500, 0));
        assert_eq!(state.projectiles[0].range_left, 8_500);
    }

    #[test]
    fn thunderbolt_hits_blastoise_for_302() {
        let mut state = new_match();
        // One tick short of Blastoise at (6000, 0).
        state
            .projectiles
            .push(shot(0, THUNDERBOLT, Vec2::new(5_000, 0), Vec2::new(500, 0)));
        let events = step(&mut state, [Intent::default(); 2]);

        assert_eq!(
            events,
            vec![Event::Hit {
                attacker: 0,
                target: 1,
                move_id: THUNDERBOLT,
                damage: 302,
                // Electric vs Water: x2, written as 8 on the x4 scale.
                effectiveness: 8,
            }]
        );
        assert_eq!(state.fighters[1].hp, 3_080 - 302);
        assert!(state.projectiles.is_empty(), "a hit removes the projectile");
    }

    #[test]
    fn water_pulse_hits_pikachu_for_270() {
        let mut state = new_match();
        state.projectiles.push(shot(
            1,
            WATER_PULSE,
            Vec2::new(-5_000, 0),
            Vec2::new(-330, 0),
        ));
        let events = step(&mut state, [Intent::default(); 2]);

        assert!(matches!(
            events[..],
            [Event::Hit {
                attacker: 1,
                target: 0,
                damage: 270,
                effectiveness: 4,
                ..
            }]
        ));
        assert_eq!(state.fighters[0].hp, 2_200 - 270);
    }

    #[test]
    fn projectile_never_hits_its_owner() {
        let mut state = new_match();
        // Starts inside Pikachu and is still overlapping it after moving.
        let pikachu = state.fighters[0].pos;
        state
            .projectiles
            .push(shot(0, THUNDERBOLT, pikachu, Vec2::new(500, 0)));
        let events = step(&mut state, [Intent::default(); 2]);

        assert!(events.is_empty());
        assert_eq!(state.fighters[0].hp, state.fighters[0].max_hp);
        assert_eq!(state.projectiles.len(), 1);
    }

    #[test]
    fn thunderbolt_fizzles_at_max_range() {
        let mut state = new_match();
        let windup = move_data(THUNDERBOLT).windup;
        step(&mut state, only_fighter_0(use_move(0)));
        // Release tick: spawned at x = -6000 and moved once.
        run(&mut state, [Intent::default(); 2], windup - 1);

        // 8500 range left at 500 per tick: still flying after 16 more ticks...
        run(&mut state, [Intent::default(); 2], 16);
        assert_eq!(state.projectiles.len(), 1);

        // ...and spent on the 17th, 9000 units from Pikachu and short of
        // Blastoise, which is 12,000 away.
        let events = run(&mut state, [Intent::default(); 2], 1);
        assert_eq!(
            events,
            vec![Event::ProjectileExpired {
                move_id: THUNDERBOLT,
                pos: Vec2::new(3_000, 0),
            }]
        );
        assert!(state.projectiles.is_empty());
        assert_eq!(state.fighters[1].hp, state.fighters[1].max_hp);
    }

    #[test]
    fn projectile_expires_when_its_edge_touches_a_wall() {
        let mut state = new_match();
        // The right wall is at x = 12,000. After moving, the center is at
        // 11,900 and the 200-unit edge reaches 12,100.
        state.projectiles.push(shot(
            0,
            THUNDERBOLT,
            Vec2::new(11_400, -5_000),
            Vec2::new(500, 0),
        ));
        let events = step(&mut state, [Intent::default(); 2]);

        assert!(matches!(events[..], [Event::ProjectileExpired { .. }]));
        assert!(state.projectiles.is_empty());
    }

    #[test]
    fn hp_never_drops_below_zero() {
        let mut state = new_match();
        state.fighters[1].hp = 100;
        state
            .projectiles
            .push(shot(0, THUNDERBOLT, Vec2::new(5_000, 0), Vec2::new(500, 0)));
        let events = step(&mut state, [Intent::default(); 2]);

        assert_eq!(state.fighters[1].hp, 0);
        // The event still reports the move's full damage. (The knockout
        // events that follow are covered by the end-of-match tests.)
        assert!(matches!(events[0], Event::Hit { damage: 302, .. }));
    }

    #[test]
    fn both_fighters_can_be_hit_on_the_same_tick() {
        let mut state = new_match();
        state
            .projectiles
            .push(shot(0, THUNDERBOLT, Vec2::new(5_000, 0), Vec2::new(500, 0)));
        state.projectiles.push(shot(
            1,
            WATER_PULSE,
            Vec2::new(-5_000, 0),
            Vec2::new(-330, 0),
        ));
        let events = step(&mut state, [Intent::default(); 2]);

        assert_eq!(events.len(), 2);
        assert_eq!(state.fighters[0].hp, 2_200 - 270);
        assert_eq!(state.fighters[1].hp, 3_080 - 302);
    }

    /// A Thunder zone placed directly in the arena.
    fn thunder_zone(owner: u8, center: Vec2, ticks_left: u16) -> Zone {
        Zone {
            owner,
            move_id: THUNDER,
            center,
            ticks_left,
        }
    }

    /// Far from both fighters, so a zone here always misses.
    const EMPTY_SPOT: Vec2 = Vec2::new(0, -6_000);

    #[test]
    fn zone_counts_down_then_lands() {
        let mut state = new_match();
        state.zones.push(thunder_zone(0, EMPTY_SPOT, 3));

        assert!(step(&mut state, [Intent::default(); 2]).is_empty());
        assert_eq!(state.zones[0].ticks_left, 2);
        assert!(step(&mut state, [Intent::default(); 2]).is_empty());
        assert_eq!(state.zones[0].ticks_left, 1);

        let events = step(&mut state, [Intent::default(); 2]);
        assert_eq!(
            events,
            vec![Event::ZoneLanded {
                owner: 0,
                move_id: THUNDER,
                center: EMPTY_SPOT,
            }]
        );
    }

    #[test]
    fn thunder_lands_exactly_delay_ticks_after_release() {
        let mut state = new_match();
        let delay = 24;
        step(&mut state, only_fighter_0(use_move(1)));
        // Release tick: placed with `delay` and already counted down once.
        run(
            &mut state,
            [Intent::default(); 2],
            move_data(THUNDER).windup - 1,
        );
        assert_eq!(state.zones[0].ticks_left, delay - 1);

        // Still waiting after delay - 2 more ticks...
        run(&mut state, [Intent::default(); 2], delay - 2);
        assert_eq!(state.zones.len(), 1);

        // ...and lands on the next, the `delay`-th tick counting the release.
        let events = run(&mut state, [Intent::default(); 2], 1);
        assert!(matches!(events[..], [Event::ZoneLanded { owner: 0, .. }]));
        assert!(state.zones.is_empty());
    }

    #[test]
    fn thunder_hits_blastoise_for_369() {
        let mut state = new_match();
        let blastoise = state.fighters[1].pos;
        state.zones.push(thunder_zone(0, blastoise, 1));
        let events = step(&mut state, [Intent::default(); 2]);

        // Landing first, then the hit.
        assert_eq!(
            events,
            vec![
                Event::ZoneLanded {
                    owner: 0,
                    move_id: THUNDER,
                    center: blastoise,
                },
                Event::Hit {
                    attacker: 0,
                    target: 1,
                    move_id: THUNDER,
                    damage: 369,
                    effectiveness: 8,
                },
            ]
        );
        assert_eq!(state.fighters[1].hp, 3_080 - 369);
    }

    #[test]
    fn zone_hits_when_only_the_body_overlaps() {
        // Thunder's radius is 1500 and Blastoise's is 550, so they touch at
        // 2050 apart. At 2000, Blastoise's center is outside the circle but
        // its body overlaps it.
        let mut state = new_match();
        state.zones.push(thunder_zone(0, Vec2::new(4_000, 0), 1));
        let events = step(&mut state, [Intent::default(); 2]);
        assert!(matches!(events[..], [_, Event::Hit { .. }]));
    }

    #[test]
    fn zone_just_touching_misses() {
        let mut state = new_match();
        state.zones.push(thunder_zone(0, Vec2::new(3_950, 0), 1));
        let events = step(&mut state, [Intent::default(); 2]);
        assert!(matches!(events[..], [Event::ZoneLanded { .. }]));
        assert_eq!(state.fighters[1].hp, state.fighters[1].max_hp);
    }

    #[test]
    fn missed_zone_lands_once_and_is_removed() {
        let mut state = new_match();
        state.zones.push(thunder_zone(0, EMPTY_SPOT, 1));

        let events = step(&mut state, [Intent::default(); 2]);
        assert!(matches!(events[..], [Event::ZoneLanded { .. }]));
        assert!(state.zones.is_empty());

        // Nothing lands a second time.
        assert!(step(&mut state, [Intent::default(); 2]).is_empty());
    }

    #[test]
    fn zone_never_hits_its_caster() {
        let mut state = new_match();
        let pikachu = state.fighters[0].pos;
        state.zones.push(thunder_zone(0, pikachu, 1));
        let events = step(&mut state, [Intent::default(); 2]);

        assert!(matches!(events[..], [Event::ZoneLanded { .. }]));
        assert_eq!(state.fighters[0].hp, state.fighters[0].max_hp);
    }

    /// Blastoise (fighter 1) mid-Aqua-Jet, heading west at 500 per tick.
    fn aqua_jet_west() -> Action {
        Action::Lunging {
            slot: 1,
            velocity: Vec2::new(-500, 0),
            ticks_left: 8,
        }
    }

    fn no_hits(events: &[Event]) -> bool {
        !events.iter().any(|e| matches!(e, Event::Hit { .. }))
    }

    #[test]
    fn missed_lunge_runs_its_full_length_then_recovers() {
        let mut state = new_match();
        let windup = move_data(AQUA_JET).windup;
        step(&mut state, [Intent::default(), use_move(1)]);
        run(&mut state, [Intent::default(); 2], windup - 1);
        assert!(matches!(
            state.fighters[1].action,
            Action::Lunging { ticks_left: 8, .. }
        ));

        // From 12,000 apart, 4,000 of lunge can't reach Pikachu.
        for _ in 0..8 {
            assert!(no_hits(&step(&mut state, [Intent::default(); 2])));
        }
        assert_eq!(state.fighters[1].pos, Vec2::new(6_000 - 4_000, 0));
        assert_eq!(
            state.fighters[1].action,
            Action::Recovering {
                ticks_left: move_data(AQUA_JET).recovery,
            }
        );
    }

    #[test]
    fn lunge_hits_on_contact_for_206_and_stops() {
        let mut state = new_match();
        // One step west of Pikachu at (-6000, 0): the bodies overlap, are
        // pushed apart to touching (900), and the 600 hitbox still reaches.
        state.fighters[1].pos = Vec2::new(-5_000, 0);
        state.fighters[1].action = aqua_jet_west();
        let events = step(&mut state, [Intent::default(); 2]);

        assert_eq!(
            events,
            vec![Event::Hit {
                attacker: 1,
                target: 0,
                move_id: AQUA_JET,
                damage: 206,
                effectiveness: 4,
            }]
        );
        assert_eq!(state.fighters[0].hp, 2_200 - 206);
        assert_eq!(
            state.fighters[1].action,
            Action::Recovering {
                ticks_left: move_data(AQUA_JET).recovery,
            },
            "a hit ends the lunge"
        );
    }

    #[test]
    fn lunge_hits_only_once() {
        let mut state = new_match();
        state.fighters[1].pos = Vec2::new(-5_000, 0);
        state.fighters[1].action = aqua_jet_west();
        step(&mut state, [Intent::default(); 2]);

        // Still touching Pikachu while recovering, but no second hit.
        for _ in 0..10 {
            assert!(no_hits(&step(&mut state, [Intent::default(); 2])));
        }
        assert_eq!(state.fighters[0].hp, 2_200 - 206);
    }

    #[test]
    fn aqua_jet_from_a_key_press_hits_a_nearby_target() {
        let mut state = new_match();
        state.fighters[1].pos = Vec2::new(-4_000, 0);
        let mut hits = Vec::new();
        step(&mut state, [Intent::default(), use_move(1)]);
        for _ in 0..12 {
            let events = step(&mut state, [Intent::default(); 2]);
            hits.extend(
                events
                    .into_iter()
                    .filter(|e| matches!(e, Event::Hit { .. })),
            );
        }
        assert!(matches!(hits[..], [Event::Hit { damage: 206, .. }]));
    }

    #[test]
    fn lunge_stops_at_the_wall() {
        let mut state = new_match();
        state.fighters[1].pos = Vec2::new(11_000, 0);
        state.fighters[1].action = Action::Lunging {
            slot: 1,
            velocity: Vec2::new(500, 0),
            ticks_left: 8,
        };
        run(&mut state, [Intent::default(); 2], 8);
        let radius = data::species(BLASTOISE).radius;
        assert_eq!(state.fighters[1].pos.x, state.arena.x - radius);
    }

    /// Puts a Thunderbolt one tick away from hitting Blastoise.
    fn thunderbolt_about_to_hit_blastoise(state: &mut State) {
        state
            .projectiles
            .push(shot(0, THUNDERBOLT, Vec2::new(5_000, 0), Vec2::new(500, 0)));
    }

    /// Puts a Water Pulse one tick away from hitting Pikachu.
    fn water_pulse_about_to_hit_pikachu(state: &mut State) {
        state.projectiles.push(shot(
            1,
            WATER_PULSE,
            Vec2::new(-5_000, 0),
            Vec2::new(-330, 0),
        ));
    }

    /// One step with no input, starting on the last tick before time runs out.
    fn step_into_timeout(state: &mut State) -> Vec<Event> {
        state.tick = MATCH_TICKS - 1;
        step(state, [Intent::default(); 2])
    }

    #[test]
    fn knockout_ends_the_match() {
        let mut state = new_match();
        state.fighters[1].hp = 100;
        thunderbolt_about_to_hit_blastoise(&mut state);
        let events = step(&mut state, [Intent::default(); 2]);

        // The hit, then the faint, then the result.
        assert!(matches!(
            events[..],
            [
                Event::Hit { .. },
                Event::Fainted { fighter: 1 },
                Event::MatchEnded {
                    outcome: Outcome::Winner(0)
                },
            ]
        ));
        assert_eq!(state.outcome, Some(Outcome::Winner(0)));
    }

    #[test]
    fn double_knockout_is_a_draw() {
        let mut state = new_match();
        state.fighters[0].hp = 100;
        state.fighters[1].hp = 100;
        thunderbolt_about_to_hit_blastoise(&mut state);
        water_pulse_about_to_hit_pikachu(&mut state);
        let events = step(&mut state, [Intent::default(); 2]);

        // Both hits land before faints are checked, so neither side wins.
        assert_eq!(
            events[2..],
            [
                Event::Fainted { fighter: 0 },
                Event::Fainted { fighter: 1 },
                Event::MatchEnded {
                    outcome: Outcome::Draw,
                },
            ]
        );
        assert_eq!(state.outcome, Some(Outcome::Draw));
    }

    #[test]
    fn match_is_frozen_after_a_knockout() {
        let mut state = new_match();
        state.fighters[1].hp = 100;
        thunderbolt_about_to_hit_blastoise(&mut state);
        step(&mut state, [Intent::default(); 2]);

        let before = state.clone();
        for _ in 0..30 {
            let events = step(&mut state, [dash(), use_move(1)]);
            assert!(events.is_empty());
        }
        assert_eq!(state, before);
    }

    #[test]
    fn match_runs_until_time_is_up() {
        let mut state = new_match();
        state.tick = MATCH_TICKS - 2;
        step(&mut state, [Intent::default(); 2]);
        assert_eq!(state.outcome, None, "one tick left");

        let events = step(&mut state, [Intent::default(); 2]);
        assert_eq!(state.tick, MATCH_TICKS);
        assert!(matches!(events[..], [Event::MatchEnded { .. }]));
    }

    #[test]
    fn timeout_higher_hp_percentage_wins() {
        let mut state = new_match();
        state.fighters[0].hp = 1_100; // 50% of 2200
        state.fighters[1].hp = 1_848; // 60% of 3080
        let events = step_into_timeout(&mut state);

        assert_eq!(
            events,
            vec![Event::MatchEnded {
                outcome: Outcome::Winner(1),
            }]
        );
        assert_eq!(state.outcome, Some(Outcome::Winner(1)));
    }

    #[test]
    fn timeout_compares_percentages_not_raw_hp() {
        let mut state = new_match();
        state.fighters[0].hp = 2_000; // about 91%
        state.fighters[1].hp = 2_500; // about 81%, but more HP
        step_into_timeout(&mut state);
        assert_eq!(state.outcome, Some(Outcome::Winner(0)));
    }

    #[test]
    fn timeout_with_equal_percentages_is_a_draw() {
        let mut state = new_match();
        state.fighters[0].hp = 1_100; // exactly 50%
        state.fighters[1].hp = 1_540; // exactly 50%
        step_into_timeout(&mut state);
        assert_eq!(state.outcome, Some(Outcome::Draw));
    }

    #[test]
    fn timeout_tells_apart_close_percentages() {
        // 1999/2200 (90.86%) vs 2800/3080 (90.91%): dividing first would call
        // this a tie; cross-multiplying sees Blastoise is slightly ahead.
        let mut state = new_match();
        state.fighters[0].hp = 1_999;
        state.fighters[1].hp = 2_800;
        step_into_timeout(&mut state);
        assert_eq!(state.outcome, Some(Outcome::Winner(1)));
    }
}
