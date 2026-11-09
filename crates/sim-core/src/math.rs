//! Integer math and units.
//!
//! Units: 1 tile = 1000 units, stored as `i32`. Speeds are units per tick.
//! Use `i64` for intermediate products so multiplication can't overflow.
//! Coordinates are screen-style: x points right, y points down.

use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

pub const UNITS_PER_TILE: i32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Vec2 {
    pub x: i32,
    pub y: i32,
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, rhs: Vec2) -> Vec2 {
        Vec2::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

impl Mul<i32> for Vec2 {
    type Output = Vec2;
    fn mul(self, rhs: i32) -> Vec2 {
        Vec2::new(self.x * rhs, self.y * rhs)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Vec2) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Vec2) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Vec2 {
    pub const ZERO: Vec2 = Vec2 { x: 0, y: 0 };

    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn dot(self, other: Vec2) -> i64 {
        let a: i64 = self.x as i64 * other.x as i64;
        let b: i64 = self.y as i64 * other.y as i64;
        a + b
    }

    pub fn length_sq(self) -> i64 {
        self.dot(self)
    }

    /// Rounded down. Fits in `i32` for any vector the game produces.
    pub fn length(self) -> i32 {
        self.length_sq().isqrt() as i32
    }

    /// Same direction, rescaled to `length`. Returns `ZERO` for a zero vector,
    /// since it has no direction to keep.
    pub fn with_length(self, length: i32) -> Vec2 {
        let magnitude: i32 = self.length();
        if magnitude == 0 {
            return Vec2::ZERO;
        }
        // Multiply before dividing, in i64, so precision isn't lost and the
        // product can't overflow.
        let x: i64 = self.x as i64 * length as i64 / magnitude as i64;
        let y: i64 = self.y as i64 * length as i64 / magnitude as i64;
        Vec2::new(x as i32, y as i32)
    }
}

/// 8 movement directions. y points down, so `N` is negative y.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir8 {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

/// `UNITS_PER_TILE / sqrt(2)`, rounded: each component of a diagonal direction.
const DIAG: i32 = 707;

impl Dir8 {
    pub const ALL: [Dir8; 8] = [
        Dir8::N,
        Dir8::NE,
        Dir8::E,
        Dir8::SE,
        Dir8::S,
        Dir8::SW,
        Dir8::W,
        Dir8::NW,
    ];

    /// Direction as a vector of length `UNITS_PER_TILE` (diagonals within 1).
    pub const fn to_vec(self) -> Vec2 {
        match self {
            Dir8::N => Vec2::new(0, -UNITS_PER_TILE),
            Dir8::NE => Vec2::new(DIAG, -DIAG),
            Dir8::E => Vec2::new(UNITS_PER_TILE, 0),
            Dir8::SE => Vec2::new(DIAG, DIAG),
            Dir8::S => Vec2::new(0, UNITS_PER_TILE),
            Dir8::SW => Vec2::new(-DIAG, DIAG),
            Dir8::W => Vec2::new(-UNITS_PER_TILE, 0),
            Dir8::NW => Vec2::new(-DIAG, -DIAG),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_operators() {
        let a = Vec2::new(3, 4);
        let b = Vec2::new(10, -2);
        assert_eq!(a + b, Vec2::new(13, 2));
        assert_eq!(a - b, Vec2::new(-7, 6));
        assert_eq!(-a, Vec2::new(-3, -4));
        assert_eq!(a * 3, Vec2::new(9, 12));
    }

    #[test]
    fn assign_operators() {
        let mut v = Vec2::new(1, 1);
        v += Vec2::new(2, 3);
        assert_eq!(v, Vec2::new(3, 4));
        v -= Vec2::new(5, 5);
        assert_eq!(v, Vec2::new(-2, -1));
    }

    #[test]
    fn dot_product() {
        assert_eq!(Vec2::new(2, 3).dot(Vec2::new(4, -5)), 8 - 15);
    }

    #[test]
    fn dot_uses_i64_for_large_values() {
        // 50_000 * 50_000 = 2.5 billion, which overflows i32.
        let v = Vec2::new(50_000, 0);
        assert_eq!(v.length_sq(), 2_500_000_000);
    }

    #[test]
    fn length_of_3_4_5() {
        assert_eq!(Vec2::new(3000, 4000).length(), 5000);
    }

    #[test]
    fn with_length_axis_aligned() {
        assert_eq!(Vec2::new(0, 5000).with_length(150), Vec2::new(0, 150));
    }

    #[test]
    fn with_length_keeps_direction() {
        assert_eq!(Vec2::new(3000, 4000).with_length(150), Vec2::new(90, 120));
    }

    #[test]
    fn with_length_diagonal() {
        assert_eq!(Vec2::new(3000, 3000).with_length(1000), Vec2::new(707, 707));
    }

    #[test]
    fn with_length_of_zero_is_zero() {
        assert_eq!(Vec2::ZERO.with_length(1000), Vec2::ZERO);
    }

    #[test]
    fn dir8_vectors_have_unit_length() {
        for dir in Dir8::ALL {
            let len = dir.to_vec().length();
            assert!(
                (len - UNITS_PER_TILE).abs() <= 1,
                "{dir:?} has length {len}"
            );
        }
    }

    #[test]
    fn dir8_north_is_negative_y() {
        assert_eq!(Dir8::N.to_vec(), Vec2::new(0, -UNITS_PER_TILE));
    }
}
