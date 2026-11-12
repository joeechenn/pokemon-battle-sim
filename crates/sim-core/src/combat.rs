//! Damage and stat formulas.
//!
//! Pure functions: data in, number out. Callers in `step` look up species and
//! moves by id and pass the data here.
//!
//! Integer math throughout. Multipliers are integer fractions (x1.5 is
//! `* 3 / 2`), inputs widen to `i64` before multiplying, and each formula
//! divides once, at the end.
//!
//! Stats use one uniform level-50 conversion for every species until
//! per-species spreads are added as data.

use crate::data::{Category, Move, Species};

pub const HP_SCALE: i32 = 20;
pub const DAMAGE_SCALE: i64 = 2;

pub fn stat(base: u16) -> i32 {
    base as i32 + 20
}

pub fn max_hp(base_hp: u16) -> i32 {
    (base_hp as i32 + 75) * HP_SCALE
}

/// HP that `mv` removes when `attacker` hits `defender`.
///
/// `power × A / D × STAB × type × DAMAGE_SCALE`, where A/D are Atk/Def for
/// physical moves and SpA/SpD for special ones. Returns 0 against an immune
/// defender, and at least 1 otherwise.
pub fn damage(attacker: &Species, defender: &Species, mv: &Move) -> i32 {
    // Type multiplier x4 (4 = neutral). Immune: skip the rest.
    let eff = mv.move_type.effectiveness(defender.types);
    if eff == 0 {
        return 0;
    }

    let (atk, def) = match mv.category {
        Category::Physical => (attacker.base.atk, defender.base.def),
        Category::Special => (attacker.base.spa, defender.base.spd),
    };
    // Base stats to battle stats, then widen everything to i64 before any math.
    let atk = i64::from(stat(atk));
    let def = i64::from(stat(def));
    let power = i64::from(mv.power);
    let eff = i64::from(eff);

    // STAB x1.5, written as x3 (or x2 without STAB); the /2 happens below.
    let stab: i64 = if attacker.types.0 == mv.move_type || attacker.types.1 == Some(mv.move_type) {
        3
    } else {
        2
    };

    // Multiply everything first, then divide once. `2 * 4` undoes the x2 in
    // `stab` and the x4 in `eff`.
    let raw = power * atk * stab * eff * DAMAGE_SCALE / (def * 2 * 4);

    i32::try_from(raw.max(1)).expect("damage fits in i32")
}

/// Walk speed in units per tick for a speed stat of 100 (4.5 tiles/s).
pub const BASE_MOVE_SPEED: i32 = 150;
/// Slowest walk speed: 25% below `BASE_MOVE_SPEED`.
const MIN_MOVE_SPEED: i32 = BASE_MOVE_SPEED * 3 / 4;
/// Fastest walk speed: 25% above `BASE_MOVE_SPEED`.
const MAX_MOVE_SPEED: i32 = BASE_MOVE_SPEED * 5 / 4;

/// Walk speed in units per tick: `BASE_MOVE_SPEED × sqrt(stat / 100)`,
/// clamped to ±25%. The square root gives diminishing returns at high Speed.
pub fn move_speed(base_spe: u16) -> i32 {
    let s = stat(base_spe);
    // x10_000 inside the root keeps two extra digits: the result is 100 × sqrt(s).
    let root = (s * 10_000).isqrt();
    let speed = BASE_MOVE_SPEED * root / 1000;
    speed.clamp(MIN_MOVE_SPEED, MAX_MOVE_SPEED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{
        AQUA_JET, BLASTOISE, BaseStats, PIKACHU, THUNDER, THUNDERBOLT, Type, WATER_PULSE,
        move_data, species,
    };

    #[test]
    fn max_hp_values() {
        assert_eq!(max_hp(species(PIKACHU).base.hp), 2200);
        assert_eq!(max_hp(species(BLASTOISE).base.hp), 3080);
    }

    #[test]
    fn damage_in_the_slice_matchup() {
        let pika = species(PIKACHU);
        let blast = species(BLASTOISE);
        assert_eq!(damage(pika, blast, move_data(THUNDERBOLT)), 302);
        assert_eq!(damage(pika, blast, move_data(THUNDER)), 369);
        assert_eq!(damage(blast, pika, move_data(WATER_PULSE)), 270);
        assert_eq!(damage(blast, pika, move_data(AQUA_JET)), 206);
    }

    #[test]
    fn immune_defender_takes_zero() {
        // Blastoise retyped to Ground: Electric can't touch it.
        let ground = Species {
            types: (Type::Ground, None),
            ..*species(BLASTOISE)
        };
        assert_eq!(damage(species(PIKACHU), &ground, move_data(THUNDERBOLT)), 0);
    }

    #[test]
    fn non_immune_hit_does_at_least_one() {
        let base = species(PIKACHU).base;
        let wall = Species {
            base: BaseStats {
                def: u16::MAX,
                ..base
            },
            ..*species(PIKACHU)
        };
        assert_eq!(damage(species(BLASTOISE), &wall, move_data(AQUA_JET)), 1);
    }

    #[test]
    fn move_speed_values() {
        assert_eq!(move_speed(species(PIKACHU).base.spe), 157);
        assert_eq!(move_speed(species(BLASTOISE).base.spe), 148);
    }

    #[test]
    fn move_speed_is_clamped() {
        assert_eq!(move_speed(5), MIN_MOVE_SPEED);
        assert_eq!(move_speed(200), MAX_MOVE_SPEED);
        assert_eq!(MIN_MOVE_SPEED, 112);
        assert_eq!(MAX_MOVE_SPEED, 187);
        // Largest possible input must not overflow.
        assert_eq!(move_speed(u16::MAX), MAX_MOVE_SPEED);
    }
}
