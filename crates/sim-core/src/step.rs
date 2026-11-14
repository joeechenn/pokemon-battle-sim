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

use crate::combat::move_speed;
use crate::data;
use crate::event::Event;
use crate::state::{Action, Fighter, Intent, State};

pub const DASH_DISTANCE: i32 = 3000;
pub const DASH_TICKS: u16 = 6;
pub const DASH_SPEED: i32 = DASH_DISTANCE / DASH_TICKS as i32;
pub const DASH_COOLDOWN: u16 = 60;
pub const DASH_RECOVERY: u16 = 3;

fn start_actions(fighters: &mut [Fighter; 2], intents: &[Intent; 2], events: &mut Vec<Event>) {
    for (i, f) in fighters.iter_mut().enumerate() {
        // Only an idle fighter can start something new.
        if f.action != Action::Idle {
            continue;
        }
        let intent = intents[i];
        if intent.dash && f.dash_cooldown == 0 {
            let direction = intent.movement.unwrap_or(f.facing);
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
    }
}

fn move_fighters(fighters: &mut [Fighter; 2], intents: &[Intent; 2]) {
    for (i, f) in fighters.iter_mut().enumerate() {
        let intent = intents[i];
        match f.action {
            Action::Idle => {
                if let Some(dir) = intent.movement {
                    let spe = data::species(f.species).base.spe;
                    let speed = move_speed(spe);
                    f.facing = dir;
                    f.pos += dir.to_vec().with_length(speed);
                }
            }
            Action::Dashing {
                velocity,
                ticks_left,
            } => {
                f.pos += velocity;
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
            // Windups count down in phase 3, where moves are released.
            Action::Casting { .. } => {}
            Action::Recovering { ticks_left } => {
                f.action = if ticks_left > 1 {
                    Action::Recovering {
                        ticks_left: ticks_left - 1,
                    }
                } else {
                    Action::Idle
                };
            }
            // Next chunk: dash-attack movement and contact hits.
            Action::Lunging { .. } => {}
        }
    }
}

pub fn step(state: &mut State, intents: [Intent; 2]) -> Vec<Event> {
    if state.outcome.is_some() {
        return Vec::new();
    }
    let mut events = Vec::new();

    start_actions(&mut state.fighters, &intents, &mut events);
    move_fighters(&mut state.fighters, &intents);

    state.tick += 1;
    events
}
