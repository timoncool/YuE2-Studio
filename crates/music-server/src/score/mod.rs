//! Scores in YuE2's two-voice ABC dialect: read into notes, carried to and from
//! MIDI and laid along lyrics. Ported from pytraveler/YuE2-ComfyUI (Apache-2.0),
//! whose notation module reads everything back with m-a-p's own reader of the dialect.

pub mod abc;
pub mod api;
pub mod edits;
pub mod export;
pub mod import;
pub mod instrumental;
pub mod notation;
pub mod phrasing;
pub mod rebuild;
pub mod schedule;
pub mod section_match;
pub mod sections;
pub mod smf;
pub mod spelling;
pub mod transpose;

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

/// An exact fraction of a quarter note. Every time and length in a score is
/// one, so no rounding ever moves a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Q {
    num: i64,
    den: i64,
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl Q {
    pub const ZERO: Q = Q { num: 0, den: 1 };
    pub const HALF: Q = Q { num: 1, den: 2 };

    pub fn new(num: i64, den: i64) -> Q {
        assert!(den != 0, "a fraction with a zero denominator");
        let sign = if den < 0 { -1 } else { 1 };
        let divisor = gcd(num, den).max(1);
        Q { num: sign * num / divisor, den: sign * den / divisor }
    }

    pub fn int(value: i64) -> Q {
        Q { num: value, den: 1 }
    }

    pub fn num(self) -> i64 {
        self.num
    }

    pub fn den(self) -> i64 {
        self.den
    }

    pub fn is_whole(self) -> bool {
        self.den == 1
    }

    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

impl fmt::Display for Q {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

impl Ord for Q {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.num as i128 * other.den as i128).cmp(&(other.num as i128 * self.den as i128))
    }
}

impl PartialOrd for Q {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Add for Q {
    type Output = Q;
    fn add(self, other: Q) -> Q {
        Q::new(self.num * other.den + other.num * self.den, self.den * other.den)
    }
}

impl AddAssign for Q {
    fn add_assign(&mut self, other: Q) {
        *self = *self + other;
    }
}

impl Sub for Q {
    type Output = Q;
    fn sub(self, other: Q) -> Q {
        Q::new(self.num * other.den - other.num * self.den, self.den * other.den)
    }
}

impl Mul for Q {
    type Output = Q;
    fn mul(self, other: Q) -> Q {
        Q::new(self.num * other.num, self.den * other.den)
    }
}

impl Div for Q {
    type Output = Q;
    fn div(self, other: Q) -> Q {
        Q::new(self.num * other.den, self.den * other.num)
    }
}

impl Neg for Q {
    type Output = Q;
    fn neg(self) -> Q {
        Q { num: -self.num, den: self.den }
    }
}

#[cfg(test)]
mod tests {
    use super::Q;

    #[test]
    fn fractions_stay_exact_and_compare_by_value() {
        let third = Q::new(1, 3);
        assert_eq!(third + third + third, Q::int(1));
        assert_eq!(Q::new(2, 4), Q::new(1, 2));
        assert!(Q::new(1, 3) < Q::new(1, 2));
        assert_eq!(Q::new(3, -6), Q::new(-1, 2));
        assert_eq!((Q::new(3, 4) / Q::new(1, 8)).to_string(), "6");
    }
}
