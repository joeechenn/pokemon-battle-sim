//! Static game data: types, species, moves.
//!
//! Read-only during a match. Hard-coded tables for the slice; later this
//! becomes the data schema the sim repo owns and loads from files.
//!
//! State refers to species and moves by id (`SpeciesId`, `MoveId`), which
//! index into `SPECIES` and `MOVES`. Game logic never names a species.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type {
    Normal,
    Fire,
    Water,
    Electric,
    Grass,
    Ice,
    Fighting,
    Poison,
    Ground,
    Flying,
    Psychic,
    Bug,
    Rock,
    Ghost,
    Dragon,
    Dark,
    Steel,
    Fairy,
}

impl Type {
    pub const COUNT: usize = 18;

    /// Multiplier x4 against a one- or two-type defender (4 = neutral)
    pub fn effectiveness(self, defender: (Type, Option<Type>)) -> u8 {
        let (primary, secondary) = defender;
        let first: u8 = TYPE_CHART[self as usize][primary as usize];
        let second: u8 = match secondary {
            Some(t) => TYPE_CHART[self as usize][t as usize],
            // no second type: neutral
            None => 2,
        };
        first * second
    }
}

/// Gen 6+ type chart. Row = attacking type, column = defending type.
/// Values are the multiplier x2: 0 = immune, 1 = x0.5, 2 = neutral, 4 = x2.
///
/// Index order (matches `Type`): 0 Normal, 1 Fire, 2 Water, 3 Electric,
/// 4 Grass, 5 Ice, 6 Fighting, 7 Poison, 8 Ground, 9 Flying, 10 Psychic,
/// 11 Bug, 12 Rock, 13 Ghost, 14 Dragon, 15 Dark, 16 Steel, 17 Fairy.
const TYPE_CHART: [[u8; Type::COUNT]; Type::COUNT] = [
    [2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 1, 0, 2, 2, 1, 2],
    [2, 1, 1, 2, 4, 4, 2, 2, 2, 2, 2, 4, 1, 2, 1, 2, 4, 2],
    [2, 4, 1, 2, 1, 2, 2, 2, 4, 2, 2, 2, 4, 2, 1, 2, 2, 2],
    [2, 2, 4, 1, 1, 2, 2, 2, 0, 4, 2, 2, 2, 2, 1, 2, 2, 2],
    [2, 1, 4, 2, 1, 2, 2, 1, 4, 1, 2, 1, 4, 2, 1, 2, 1, 2],
    [2, 1, 1, 2, 4, 1, 2, 2, 4, 4, 2, 2, 2, 2, 4, 2, 1, 2],
    [4, 2, 2, 2, 2, 4, 2, 1, 2, 1, 1, 1, 4, 0, 2, 4, 4, 1],
    [2, 2, 2, 2, 4, 2, 2, 1, 1, 2, 2, 2, 1, 1, 2, 2, 0, 4],
    [2, 4, 2, 4, 1, 2, 2, 4, 2, 0, 2, 1, 4, 2, 2, 2, 4, 2],
    [2, 2, 2, 1, 4, 2, 4, 2, 2, 2, 2, 4, 1, 2, 2, 2, 1, 2],
    [2, 2, 2, 2, 2, 2, 4, 4, 2, 2, 1, 2, 2, 2, 2, 0, 1, 2],
    [2, 1, 2, 2, 4, 2, 1, 1, 2, 1, 4, 2, 2, 1, 2, 4, 1, 1],
    [2, 4, 2, 2, 2, 4, 1, 2, 1, 4, 2, 4, 2, 2, 2, 2, 1, 2],
    [0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 4, 2, 2, 4, 2, 1, 2, 2],
    [2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 4, 2, 1, 0],
    [2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 4, 2, 2, 4, 2, 1, 2, 1],
    [2, 1, 1, 1, 2, 4, 2, 2, 2, 2, 2, 2, 4, 2, 2, 2, 1, 4],
    [2, 1, 2, 2, 2, 2, 4, 1, 2, 2, 2, 2, 2, 2, 4, 4, 1, 2],
];

/// Index into `SPECIES`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeciesId(pub u16);

/// Index into `MOVES`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveId(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseStats {
    pub hp: u16,
    pub atk: u16,
    pub def: u16,
    pub spa: u16,
    pub spd: u16,
    pub spe: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Species {
    pub name: &'static str,
    pub types: (Type, Option<Type>),
    pub base: BaseStats,
    /// Body hitbox radius in units
    pub radius: i32,
    pub moves: [MoveId; 2],
}

/// Physical moves use Atk vs Def; special moves use SpA vs SpD
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Physical,
    Special,
}

/// How a move exists in the arena. Distances in units, durations in ticks
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Archetype {
    /// Flies in a straight line and hits the first fighter it touches
    Projectile {
        speed: i32,
        radius: i32,
        range: i32
    },
    /// A telegraphed circle placed up to `cast_range` away; lands after `delay`
    Zone {
        radius: i32,
        delay: u16,
        cast_range: i32,
    },
    /// The user lunges `distance` over `duration` ticks and hits on contact
    DashAttack {
        distance: i32,
        duration: u16,
        radius: i32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub name: &'static str,
    pub move_type: Type,
    pub category: Category,
    pub power: u16,
    /// Ticks from command until the move is released
    pub windup: u16,
    /// Ticks after release before the user can act again
    pub recovery: u16,
    /// Ticks from release until the move can be used again
    pub cooldown: u16,
    pub archetype: Archetype,
}

pub const PIKACHU: SpeciesId = SpeciesId(0);
pub const BLASTOISE: SpeciesId = SpeciesId(1);

pub const THUNDERBOLT: MoveId = MoveId(0);
pub const THUNDER: MoveId = MoveId(1);
pub const WATER_PULSE: MoveId = MoveId(2);
pub const AQUA_JET: MoveId = MoveId(3);

/// Entry `i` is the species with `SpeciesId(i)
pub static SPECIES: [Species; 2] = [
    Species {
        name: "Pikachu",
        types: (Type::Electric, None),
        base: BaseStats {
            hp: 35,
            atk: 55,
            def: 40,
            spa: 50,
            spd: 50,
            spe: 90,
        },
        radius: 350,
        moves: [THUNDERBOLT, THUNDER],
    },
    Species {
        name: "Blastoise",
        types: (Type::Water, None),
        base: BaseStats {
            hp: 79,
            atk: 83,
            def: 100,
            spa: 85,
            spd: 105,
            spe: 78,
        },
        radius: 550,
        moves: [WATER_PULSE, AQUA_JET],
    },
];

/// Entry `i` is the move with `MoveId(i)`. Timing and geometry are starting
/// values to tune in the viewer.
pub static MOVES: [Move; 4] = [
    Move {
        name: "Thunderbolt",
        move_type: Type::Electric,
        category: Category::Special,
        power: 90,
        windup: 6,
        recovery: 6,
        cooldown: 45,
        archetype: Archetype::Projectile {
            speed: 500,
            radius: 200,
            range: 9000,
        },
    },
    Move {
        name: "Thunder",
        move_type: Type::Electric,
        category: Category::Special,
        power: 110,
        windup: 15,
        recovery: 10,
        cooldown: 150,
        archetype: Archetype::Zone {
            radius: 1500,
            delay: 24,
            cast_range: 8000,
        },
    },
    Move {
        name: "Water Pulse",
        move_type: Type::Water,
        category: Category::Special,
        power: 60,
        windup: 8,
        recovery: 8,
        cooldown: 60,
        archetype: Archetype::Projectile {
            speed: 330,
            radius: 350,
            range: 8000,
        },
    },
    Move {
        name: "Aqua Jet",
        move_type: Type::Water,
        category: Category::Physical,
        power: 40,
        windup: 3,
        recovery: 10,
        cooldown: 90,
        archetype: Archetype::DashAttack {
            distance: 4000,
            duration: 8,
            radius: 600,
        },
    },
];

pub fn species(id: SpeciesId) -> &'static Species {
    &SPECIES[id.0 as usize]
}

pub fn move_data(id: MoveId) -> &'static Move {
    &MOVES[id.0 as usize]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookups_return_matching_entries() {
        assert_eq!(species(PIKACHU).name, "Pikachu");
        assert_eq!(species(BLASTOISE).name, "Blastoise");
        assert_eq!(move_data(THUNDERBOLT).name, "Thunderbolt");
        assert_eq!(move_data(THUNDER).name, "Thunder");
        assert_eq!(move_data(WATER_PULSE).name, "Water Pulse");
        assert_eq!(move_data(AQUA_JET).name, "Aqua Jet");
    }

    #[test]
    fn every_species_move_exists() {
        for s in &SPECIES {
            for m in s.moves {
                assert!(
                    (m.0 as usize) < MOVES.len(),
                    "{} has bad move {m:?}",
                    s.name
                );
            }
        }
    }

    #[test]
    fn single_type_effectiveness() {
        assert_eq!(Type::Electric.effectiveness((Type::Water, None)), 8);
        assert_eq!(Type::Electric.effectiveness((Type::Ground, None)), 0);
        assert_eq!(Type::Water.effectiveness((Type::Water, None)), 2);
        assert_eq!(Type::Normal.effectiveness((Type::Normal, None)), 4);
    }

    #[test]
    fn dual_type_effectiveness() {
        // Gyarados: x2 * x2 = x4
        assert_eq!(
            Type::Electric.effectiveness((Type::Water, Some(Type::Flying))),
            16
        );
        // Swampert: Ground immunity wins
        assert_eq!(
            Type::Electric.effectiveness((Type::Water, Some(Type::Ground))),
            0
        );
        // x0.5 * x0.5 = x0.25
        assert_eq!(Type::Fire.effectiveness((Type::Water, Some(Type::Rock))), 1);
    }
}
