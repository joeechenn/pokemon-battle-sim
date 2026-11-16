//! Desktop debug viewer for the battle sim.
//!
//! Draws the match with simple shapes and drives one fighter from the
//! keyboard. Real time lives here, not in the sim: the window redraws every
//! frame, while the sim advances in fixed ticks counted with an accumulator.
//! Floats are fine in this crate because nothing here feeds back into the sim.
//!
//! Controls:
//!   WASD / arrows  move              Space  dash
//!   J / K          use move 1 / 2    Tab    switch controlled fighter
//!   P              pause / resume    N      step one tick while paused
//!   R              restart with a new seed

use std::collections::VecDeque;

// The explicit `Vec2` import below shadows macroquad's own `Vec2` from the
// glob, so `Vec2` always means the sim's integer vector in this file.
use macroquad::prelude::*;
use sim_core::TICKS_PER_SECOND;
use sim_core::data::{self, Archetype, BLASTOISE, PIKACHU};
use sim_core::event::Event;
use sim_core::math::{Dir8, UNITS_PER_TILE, Vec2};
use sim_core::state::{Action, Fighter, Intent, State};
use sim_core::step::step;

/// Real seconds per sim tick.
const TICK_SECONDS: f32 = 1.0 / TICKS_PER_SECOND as f32;
/// Cap on ticks per frame, so a long stall (such as dragging the window)
/// drops time instead of making the sim race to catch up.
const MAX_TICKS_PER_FRAME: u32 = 5;
/// How many recent events the side panel lists.
const EVENT_LOG_LEN: usize = 12;

const MARGIN: f32 = 24.0;
const PANEL_WIDTH: f32 = 400.0;
const LINE: f32 = 22.0;
const FONT: f32 = 20.0;

const BACKGROUND: Color = Color::new(0.09, 0.10, 0.13, 1.0);
const GRID: Color = Color::new(1.0, 1.0, 1.0, 0.05);
const TEXT: Color = Color::new(0.85, 0.87, 0.90, 1.0);
const DIM: Color = Color::new(0.55, 0.58, 0.63, 1.0);
const FIGHTER_COLORS: [Color; 2] = [
    Color::new(0.98, 0.84, 0.25, 1.0),
    Color::new(0.35, 0.62, 0.98, 1.0),
];

fn window_conf() -> Conf {
    Conf {
        window_title: "Battle sim viewer".to_owned(),
        window_width: 1400,
        window_height: 820,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let mut viewer = Viewer::new(1);
    loop {
        viewer.handle_keys();
        viewer.advance(get_frame_time());
        viewer.draw();
        next_frame().await;
    }
}

struct Viewer {
    state: State,
    seed: u64,
    /// Index of the fighter the keyboard controls.
    me: usize,
    paused: bool,
    /// Real time not yet turned into ticks.
    accumulator: f32,
    /// Presses are remembered until the next tick, so a quick tap between two
    /// ticks isn't lost.
    dash_queued: bool,
    move_queued: Option<u8>,
    /// Recent events with the tick they happened on, oldest first.
    log: VecDeque<(u32, Event)>,
}

impl Viewer {
    fn new(seed: u64) -> Self {
        Self {
            state: State::new(seed, [PIKACHU, BLASTOISE]),
            seed,
            me: 0,
            paused: false,
            accumulator: 0.0,
            dash_queued: false,
            move_queued: None,
            log: VecDeque::new(),
        }
    }

    fn handle_keys(&mut self) {
        if is_key_pressed(KeyCode::Space) {
            self.dash_queued = true;
        }
        if is_key_pressed(KeyCode::J) {
            self.move_queued = Some(0);
        }
        if is_key_pressed(KeyCode::K) {
            self.move_queued = Some(1);
        }
        if is_key_pressed(KeyCode::Tab) {
            self.me = 1 - self.me;
        }
        if is_key_pressed(KeyCode::P) {
            self.paused = !self.paused;
        }
        if is_key_pressed(KeyCode::N) && self.paused {
            self.tick();
        }
        if is_key_pressed(KeyCode::R) {
            let me = self.me;
            *self = Viewer::new(self.seed + 1);
            self.me = me;
        }
    }

    /// Runs as many whole ticks as the elapsed real time covers.
    fn advance(&mut self, frame_seconds: f32) {
        if self.paused || self.state.outcome.is_some() {
            self.accumulator = 0.0;
            return;
        }
        self.accumulator += frame_seconds;
        let mut ticks = 0;
        while self.accumulator >= TICK_SECONDS && ticks < MAX_TICKS_PER_FRAME {
            self.tick();
            self.accumulator -= TICK_SECONDS;
            ticks += 1;
        }
        if ticks == MAX_TICKS_PER_FRAME {
            self.accumulator = 0.0;
        }
    }

    /// Builds this tick's intents from the keyboard and steps the sim once.
    fn tick(&mut self) {
        let mut intents = [Intent::default(); 2];
        intents[self.me] = Intent {
            movement: held_direction(),
            // `take` reads the queued press and resets it in one go.
            dash: std::mem::take(&mut self.dash_queued),
            move_slot: self.move_queued.take(),
        };

        let tick = self.state.tick;
        for event in step(&mut self.state, intents) {
            self.log.push_back((tick, event));
            if self.log.len() > EVENT_LOG_LEN {
                self.log.pop_front();
            }
        }
    }

    fn draw(&self) {
        clear_background(BACKGROUND);
        let view = View::fit(self.state.arena);
        self.draw_arena(&view);
        self.draw_zones(&view);
        self.draw_projectiles(&view);
        for (i, fighter) in self.state.fighters.iter().enumerate() {
            draw_fighter(&view, fighter, FIGHTER_COLORS[i], i == self.me);
        }
        self.draw_panel();
    }

    fn draw_arena(&self, view: &View) {
        let arena = self.state.arena;
        // One grid line per tile.
        for x in (-arena.x..=arena.x).step_by(UNITS_PER_TILE as usize) {
            let (sx, top) = view.point(Vec2::new(x, -arena.y));
            let (_, bottom) = view.point(Vec2::new(x, arena.y));
            draw_line(sx, top, sx, bottom, 1.0, GRID);
        }
        for y in (-arena.y..=arena.y).step_by(UNITS_PER_TILE as usize) {
            let (left, sy) = view.point(Vec2::new(-arena.x, y));
            let (right, _) = view.point(Vec2::new(arena.x, y));
            draw_line(left, sy, right, sy, 1.0, GRID);
        }
        let (x, y) = view.point(-arena);
        draw_rectangle_lines(x, y, view.len(2 * arena.x), view.len(2 * arena.y), 3.0, DIM);
    }

    /// Telegraphs: an outline of the full area, filling in as it gets closer
    /// to landing.
    fn draw_zones(&self, view: &View) {
        for zone in &self.state.zones {
            let Archetype::Zone { radius, delay, .. } = data::move_data(zone.move_id).archetype
            else {
                continue;
            };
            let (x, y) = view.point(zone.center);
            let color = FIGHTER_COLORS[usize::from(zone.owner)];
            let progress = 1.0 - f32::from(zone.ticks_left) / f32::from(delay.max(1));
            draw_circle(
                x,
                y,
                view.len(radius),
                with_alpha(color, 0.10 + 0.35 * progress),
            );
            draw_circle_lines(x, y, view.len(radius), 2.0, color);
        }
    }

    fn draw_projectiles(&self, view: &View) {
        for projectile in &self.state.projectiles {
            let Archetype::Projectile { radius, .. } =
                data::move_data(projectile.move_id).archetype
            else {
                continue;
            };
            let (x, y) = view.point(projectile.pos);
            let color = FIGHTER_COLORS[usize::from(projectile.owner)];
            draw_circle(x, y, view.len(radius), color);
        }
    }

    fn draw_panel(&self) {
        let x = screen_width() - PANEL_WIDTH;
        let mut y = MARGIN + FONT;
        let mut line = |text: &str, color: Color| {
            draw_text(text, x, y, FONT, color);
            y += LINE;
        };

        let tick = self.state.tick;
        line(
            &format!("tick {tick}  ({:.1} s)", tick as f32 * TICK_SECONDS),
            TEXT,
        );
        line(&format!("seed {}", self.seed), DIM);
        if self.paused {
            line("PAUSED  (N = step one tick)", FIGHTER_COLORS[0]);
        }
        if let Some(outcome) = self.state.outcome {
            line(&format!("match over: {outcome:?}"), FIGHTER_COLORS[0]);
        }
        line("", TEXT);

        for (i, f) in self.state.fighters.iter().enumerate() {
            let species = data::species(f.species);
            let marker = if i == self.me { "  < you" } else { "" };
            line(&format!("{}{marker}", species.name), FIGHTER_COLORS[i]);
            line(&format!("  hp {} / {}", f.hp, f.max_hp), TEXT);
            line(&format!("  {}", action_label(f.action)), TEXT);
            line(
                &format!(
                    "  dash cd {}   move cd {} / {}",
                    f.dash_cooldown, f.move_cooldowns[0], f.move_cooldowns[1]
                ),
                DIM,
            );
        }
        line("", TEXT);

        line("events", TEXT);
        for (tick, event) in &self.log {
            line(&format!("  {tick:>5}  {}", self.event_label(event)), DIM);
        }
        line("", TEXT);

        let mine = data::species(self.state.fighters[self.me].species);
        let move_name = |slot: usize| data::move_data(mine.moves[slot]).name;
        line("WASD/arrows move   Space dash", DIM);
        line(&format!("J {}   K {}", move_name(0), move_name(1)), DIM);
        line("Tab switch fighter   P pause   R restart", DIM);
    }

    /// Short, readable form of an event for the log, with fighters and moves
    /// shown by name.
    fn event_label(&self, event: &Event) -> String {
        let fighter = |i: u8| data::species(self.state.fighters[usize::from(i)].species).name;
        let mv = |id| data::move_data(id).name;
        match *event {
            Event::MoveStarted {
                fighter: f,
                move_id,
            } => {
                format!("{} uses {}", fighter(f), mv(move_id))
            }
            Event::ProjectileSpawned { move_id, .. } => format!("{} fired", mv(move_id)),
            Event::ProjectileExpired { move_id, .. } => format!("{} fizzled", mv(move_id)),
            Event::TelegraphStarted { move_id, delay, .. } => {
                format!("{} lands in {delay} ticks", mv(move_id))
            }
            Event::ZoneLanded { move_id, .. } => format!("{} landed", mv(move_id)),
            Event::Dashed {
                fighter: f,
                direction,
            } => format!("{} dashed {direction:?}", fighter(f)),
            Event::Hit {
                target,
                move_id,
                damage,
                effectiveness,
                ..
            } => format!(
                "{} hit {} for {damage}{}",
                mv(move_id),
                fighter(target),
                effectiveness_label(effectiveness)
            ),
            Event::Fainted { fighter: f } => format!("{} fainted", fighter(f)),
            Event::MatchEnded { outcome } => format!("match over: {outcome:?}"),
        }
    }
}

/// Type multiplier suffix for hit labels. `effectiveness` is the x4 scale
/// from `Type::effectiveness` (4 = neutral, shown as nothing).
fn effectiveness_label(effectiveness: u8) -> &'static str {
    match effectiveness {
        0 => " (immune)",
        1 => " (x0.25)",
        2 => " (x0.5)",
        8 => " (x2)",
        16 => " (x4)",
        _ => "",
    }
}

/// Converts sim units to screen pixels: the arena is scaled to fit the space
/// left of the side panel, and centered in it.
struct View {
    scale: f32,
    center_x: f32,
    center_y: f32,
}

impl View {
    fn fit(arena: Vec2) -> Self {
        let width = screen_width() - PANEL_WIDTH - 2.0 * MARGIN;
        let height = screen_height() - 2.0 * MARGIN;
        let scale = (width / (2 * arena.x) as f32).min(height / (2 * arena.y) as f32);
        Self {
            scale,
            center_x: MARGIN + width / 2.0,
            center_y: MARGIN + height / 2.0,
        }
    }

    fn point(&self, p: Vec2) -> (f32, f32) {
        (
            self.center_x + p.x as f32 * self.scale,
            self.center_y + p.y as f32 * self.scale,
        )
    }

    fn len(&self, units: i32) -> f32 {
        units as f32 * self.scale
    }
}

fn draw_fighter(view: &View, f: &Fighter, color: Color, controlled: bool) {
    let radius = data::species(f.species).radius;
    let (x, y) = view.point(f.pos);
    let r = view.len(radius);

    draw_circle(x, y, r, with_alpha(color, 0.35));
    draw_circle_lines(x, y, r, 2.0, color);
    if controlled {
        draw_circle_lines(x, y, r + 4.0, 1.5, WHITE);
    }

    // Facing: a line from the center to the edge of the body.
    let tip = f.pos + f.facing.to_vec().with_length(radius);
    let (tx, ty) = view.point(tip);
    draw_line(x, y, tx, ty, 3.0, color);

    // HP bar above the body.
    let bar_width = 2.0 * r;
    let bar_y = y - r - 12.0;
    let fraction = f.hp as f32 / f.max_hp.max(1) as f32;
    draw_rectangle(x - r, bar_y, bar_width, 6.0, Color::new(0.2, 0.2, 0.2, 1.0));
    draw_rectangle(x - r, bar_y, bar_width * fraction, 6.0, color);
}

/// Short, readable form of an action for the side panel.
fn action_label(action: Action) -> String {
    match action {
        Action::Idle => "Idle".to_owned(),
        Action::Dashing { ticks_left, .. } => format!("Dashing ({ticks_left} left)"),
        Action::Casting { slot, ticks_left } => {
            format!("Casting move {} ({ticks_left} left)", slot + 1)
        }
        Action::Recovering { ticks_left } => format!("Recovering ({ticks_left} left)"),
        Action::Lunging { ticks_left, .. } => format!("Lunging ({ticks_left} left)"),
    }
}

/// The direction the held keys point in, or `None` if no keys (or only
/// opposite keys) are held. Screen y points down, so "up" is north.
fn held_direction() -> Option<Dir8> {
    let held = |a: KeyCode, b: KeyCode| is_key_down(a) || is_key_down(b);
    let mut dx = 0;
    let mut dy = 0;
    if held(KeyCode::A, KeyCode::Left) {
        dx -= 1;
    }
    if held(KeyCode::D, KeyCode::Right) {
        dx += 1;
    }
    if held(KeyCode::W, KeyCode::Up) {
        dy -= 1;
    }
    if held(KeyCode::S, KeyCode::Down) {
        dy += 1;
    }
    match (dx, dy) {
        (0, -1) => Some(Dir8::N),
        (1, -1) => Some(Dir8::NE),
        (1, 0) => Some(Dir8::E),
        (1, 1) => Some(Dir8::SE),
        (0, 1) => Some(Dir8::S),
        (-1, 1) => Some(Dir8::SW),
        (-1, 0) => Some(Dir8::W),
        (-1, -1) => Some(Dir8::NW),
        _ => None,
    }
}

fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}
