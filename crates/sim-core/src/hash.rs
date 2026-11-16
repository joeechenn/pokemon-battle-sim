//! Stable per-tick state hash for determinism tests.
//!
//! Two runs that should be identical are compared by hashing the state after
//! every tick: one mismatch pinpoints the first tick where they diverged.
//!
//! Why not `#[derive(Hash)]` with Rust's built-in hasher:
//! - `DefaultHasher` may change between compiler versions.
//! - Derived `Hash` writes enum tags and `Vec` lengths as `usize`/`isize`,
//!   which are 8 bytes on 64-bit native builds but 4 bytes on WASM, so the
//!   same state would hash differently on the two platforms.
//!
//! So this hashes every field by hand, in a fixed order, as fixed-width
//! little-endian bytes, with FNV-1a (a tiny, well-known, stable hash).

use crate::math::Vec2;
use crate::state::{Action, Fighter, Outcome, Projectile, State, Zone};

/// Hash of everything in `state` that can affect the rest of the match.
pub fn hash_state(state: &State) -> u64 {
    let mut h = Fnv::new();
    h.state(state);
    h.finish()
}

/// One hash for a whole run: combines per-tick hashes in order.
pub fn combine(hashes: &[u64]) -> u64 {
    let mut h = Fnv::new();
    for &x in hashes {
        h.u64(x);
    }
    h.finish()
}

/// FNV-1a, 64-bit: for each byte, XOR it in, then multiply by a fixed prime.
struct Fnv(u64);

impl Fnv {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET_BASIS)
    }

    fn finish(&self) -> u64 {
        self.0
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            // Overflow is part of the algorithm (math mod 2^64).
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }
    fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }
    fn vec2(&mut self, v: Vec2) {
        self.i32(v.x);
        self.i32(v.y);
    }
    /// Collection lengths as a fixed-width `u32`, never `usize`.
    fn len(&mut self, len: usize) {
        self.u32(u32::try_from(len).expect("collection too large to hash"));
    }

    // Each `let Type { .. } = value;` below names every field with no `..`
    // rest pattern. Adding a field to one of these types then fails to compile
    // here until the new field is hashed too, so the hash can't silently go
    // stale.

    fn state(&mut self, s: &State) {
        let State {
            tick,
            rng,
            arena,
            fighters,
            projectiles,
            zones,
            outcome,
        } = s;
        self.u32(*tick);
        self.u64(rng.state_bits());
        self.vec2(*arena);
        for f in fighters {
            self.fighter(f);
        }
        self.len(projectiles.len());
        for p in projectiles {
            self.projectile(p);
        }
        self.len(zones.len());
        for z in zones {
            self.zone(z);
        }
        self.outcome(*outcome);
    }

    fn fighter(&mut self, f: &Fighter) {
        let Fighter {
            species,
            pos,
            facing,
            hp,
            max_hp,
            move_cooldowns,
            dash_cooldown,
            action,
        } = f;
        self.u16(species.0);
        self.vec2(*pos);
        // A fieldless enum casts to its variant number: N = 0, NE = 1, ...
        self.u8(*facing as u8);
        self.i32(*hp);
        self.i32(*max_hp);
        for c in move_cooldowns {
            self.u16(*c);
        }
        self.u16(*dash_cooldown);
        self.action(*action);
    }

    /// A tag byte for the variant, then that variant's fields.
    fn action(&mut self, a: Action) {
        match a {
            Action::Idle => self.u8(0),
            Action::Dashing {
                velocity,
                ticks_left,
            } => {
                self.u8(1);
                self.vec2(velocity);
                self.u16(ticks_left);
            }
            Action::Casting { slot, ticks_left } => {
                self.u8(2);
                self.u8(slot);
                self.u16(ticks_left);
            }
            Action::Recovering { ticks_left } => {
                self.u8(3);
                self.u16(ticks_left);
            }
            Action::Lunging {
                slot,
                velocity,
                ticks_left,
            } => {
                self.u8(4);
                self.u8(slot);
                self.vec2(velocity);
                self.u16(ticks_left);
            }
        }
    }

    fn projectile(&mut self, p: &Projectile) {
        let Projectile {
            owner,
            move_id,
            pos,
            velocity,
            range_left,
        } = p;
        self.u8(*owner);
        self.u16(move_id.0);
        self.vec2(*pos);
        self.vec2(*velocity);
        self.i32(*range_left);
    }

    fn zone(&mut self, z: &Zone) {
        let Zone {
            owner,
            move_id,
            center,
            ticks_left,
        } = z;
        self.u8(*owner);
        self.u16(move_id.0);
        self.vec2(*center);
        self.u16(*ticks_left);
    }

    fn outcome(&mut self, o: Option<Outcome>) {
        match o {
            None => self.u8(0),
            Some(Outcome::Draw) => self.u8(1),
            Some(Outcome::Winner(i)) => {
                self.u8(2);
                self.u8(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{BLASTOISE, PIKACHU, THUNDERBOLT};

    fn new_match() -> State {
        State::new(42, [PIKACHU, BLASTOISE])
    }

    #[test]
    fn fnv_matches_published_test_vectors() {
        // Reference values for FNV-1a 64 from the algorithm's authors.
        let hash = |s: &str| {
            let mut h = Fnv::new();
            h.bytes(s.as_bytes());
            h.finish()
        };
        assert_eq!(hash(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(hash("a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(hash("foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn equal_states_hash_equal() {
        assert_eq!(hash_state(&new_match()), hash_state(&new_match()));
    }

    #[test]
    fn every_kind_of_change_changes_the_hash() {
        let base = hash_state(&new_match());
        let changes: [fn(&mut State); 9] = [
            |s| s.tick += 1,
            |s| {
                s.rng.next_u64();
            },
            |s| s.fighters[0].pos.x += 1,
            |s| s.fighters[1].hp -= 1,
            |s| s.fighters[0].move_cooldowns[1] = 1,
            |s| s.fighters[0].action = Action::Recovering { ticks_left: 1 },
            |s| {
                s.projectiles.push(Projectile {
                    owner: 0,
                    move_id: THUNDERBOLT,
                    pos: Vec2::ZERO,
                    velocity: Vec2::ZERO,
                    range_left: 0,
                })
            },
            |s| s.outcome = Some(Outcome::Draw),
            |s| s.fighters.swap(0, 1),
        ];
        for (i, change) in changes.iter().enumerate() {
            let mut s = new_match();
            change(&mut s);
            assert_ne!(hash_state(&s), base, "change {i} didn't affect the hash");
        }
    }
}
