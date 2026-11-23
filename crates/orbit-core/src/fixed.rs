//! Fixed-point arithmetic with 7 decimal places, matching Stellar's stroop precision.
//!
//! Every value that ends up in a published depth figure goes through this type. Floating
//! point is never used for money: results must be reproducible bit-for-bit from a stored
//! snapshot, and rounding must always go *down* (towards less depth), never up.

use core::fmt;
use core::str::FromStr;
use serde::{Deserialize, Serialize};

use crate::error::CoreError;

/// Number of decimal places carried by [`Fx`].
pub const DECIMALS: u32 = 7;
/// `10^DECIMALS`.
pub const SCALE: i128 = 10_000_000;

/// A non-negative fixed-point number with 7 decimals, stored as an integer count of
/// 1e-7 units. Construction rejects negative values; arithmetic is checked.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Fx(i128);

impl Fx {
    pub const ZERO: Fx = Fx(0);
    pub const ONE: Fx = Fx(SCALE);

    /// Whole-number constant for compile-time thresholds. Only for non-negative literals.
    pub(crate) const fn whole(n: u32) -> Fx {
        Fx(n as i128 * SCALE)
    }

    /// Builds from raw 1e-7 units (stroops).
    pub fn from_raw(raw: i128) -> Result<Self, CoreError> {
        if raw < 0 {
            return Err(CoreError::Negative);
        }
        Ok(Fx(raw))
    }

    /// Builds from a whole number.
    pub fn from_int(n: i128) -> Result<Self, CoreError> {
        n.checked_mul(SCALE)
            .ok_or(CoreError::Overflow)
            .and_then(Fx::from_raw)
    }

    pub fn raw(self) -> i128 {
        self.0
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn checked_add(self, rhs: Fx) -> Result<Fx, CoreError> {
        self.0.checked_add(rhs.0).map(Fx).ok_or(CoreError::Overflow)
    }

    /// Subtraction that fails rather than going negative.
    pub fn checked_sub(self, rhs: Fx) -> Result<Fx, CoreError> {
        match self.0.checked_sub(rhs.0) {
            Some(v) if v >= 0 => Ok(Fx(v)),
            Some(_) => Err(CoreError::Negative),
            None => Err(CoreError::Overflow),
        }
    }

    /// Multiplication, rounded down.
    pub fn mul_down(self, rhs: Fx) -> Result<Fx, CoreError> {
        let p = self.0.checked_mul(rhs.0).ok_or(CoreError::Overflow)?;
        Ok(Fx(p / SCALE))
    }

    /// Division, rounded down.
    pub fn div_down(self, rhs: Fx) -> Result<Fx, CoreError> {
        if rhs.0 == 0 {
            return Err(CoreError::DivByZero);
        }
        let n = self.0.checked_mul(SCALE).ok_or(CoreError::Overflow)?;
        Ok(Fx(n / rhs.0))
    }

    /// `self * num / den` with a single rounding (down). Falls back to dividing first when
    /// the intermediate product would overflow, which only loses precision, never safety.
    pub fn mul_div_down(self, num: Fx, den: Fx) -> Result<Fx, CoreError> {
        if den.0 == 0 {
            return Err(CoreError::DivByZero);
        }
        match self.0.checked_mul(num.0) {
            Some(p) => Fx::from_raw(p / den.0),
            None => self.mul_down(num.div_down(den)?),
        }
    }

    /// Division, rounded up. Used where rounding down would overstate depth.
    pub fn div_up(self, rhs: Fx) -> Result<Fx, CoreError> {
        if rhs.0 == 0 {
            return Err(CoreError::DivByZero);
        }
        let n = self.0.checked_mul(SCALE).ok_or(CoreError::Overflow)?;
        let q = n / rhs.0;
        Ok(Fx(if n % rhs.0 == 0 { q } else { q + 1 }))
    }

    /// `self * bps / 10_000`, rounded down. `bps` is basis points.
    pub fn mul_bps_down(self, bps: u32) -> Result<Fx, CoreError> {
        let p = self
            .0
            .checked_mul(i128::from(bps))
            .ok_or(CoreError::Overflow)?;
        Ok(Fx(p / 10_000))
    }

    pub fn min(self, other: Fx) -> Fx {
        if self <= other {
            self
        } else {
            other
        }
    }
}

impl FromStr for Fx {
    type Err = CoreError;

    /// Parses a decimal string such as `"1.0805088"` (the format Horizon uses).
    /// Rejects signs, exponents and more than 7 decimal places, so a value is never
    /// silently truncated.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(CoreError::Parse(s.to_string()));
        }
        let (int_part, frac_part) = match s.split_once('.') {
            Some((i, f)) => (i, f),
            None => (s, ""),
        };
        let all_digits = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
        if int_part.is_empty() || !all_digits(int_part) || !all_digits(frac_part) {
            return Err(CoreError::Parse(s.to_string()));
        }
        if frac_part.len() > DECIMALS as usize {
            return Err(CoreError::Precision(s.to_string()));
        }
        let int_val: i128 = int_part
            .parse()
            .map_err(|_| CoreError::Parse(s.to_string()))?;
        let mut frac_val: i128 = 0;
        for b in frac_part.bytes() {
            frac_val = frac_val * 10 + i128::from(b - b'0');
        }
        for _ in frac_part.len()..DECIMALS as usize {
            frac_val *= 10;
        }
        let raw = int_val
            .checked_mul(SCALE)
            .and_then(|v| v.checked_add(frac_val))
            .ok_or(CoreError::Overflow)?;
        Fx::from_raw(raw)
    }
}

impl fmt::Display for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:07}", self.0 / SCALE, self.0 % SCALE)
    }
}

impl fmt::Debug for Fx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fx({self})")
    }
}

impl TryFrom<String> for Fx {
    type Error = CoreError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl From<Fx> for String {
    fn from(v: Fx) -> String {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> Fx {
        s.parse().unwrap()
    }

    #[test]
    fn parses_horizon_strings() {
        assert_eq!(fx("1.0805088").raw(), 10_805_088);
        assert_eq!(fx("268312.6794594").raw(), 2_683_126_794_594);
        assert_eq!(fx("5").raw(), 50_000_000);
        assert_eq!(fx("0.0000001").raw(), 1);
    }

    #[test]
    fn rejects_bad_input() {
        for bad in ["", "-1", "1e5", "1.00000001", ".5", "1.2.3", "abc", "+1"] {
            assert!(bad.parse::<Fx>().is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn round_trips_display() {
        for s in ["0.0000000", "1.0805088", "268312.6794594"] {
            assert_eq!(fx(s).to_string(), s);
        }
    }

    #[test]
    fn arithmetic_rounds_down() {
        // 1/3 = 0.3333333 (down), never 0.3333334
        assert_eq!(Fx::ONE.div_down(fx("3")).unwrap(), fx("0.3333333"));
        assert_eq!(fx("0.0000001").mul_down(fx("0.5")).unwrap(), Fx::ZERO);
        assert_eq!(fx("100").mul_bps_down(1_000).unwrap(), fx("10"));
    }

    #[test]
    fn checked_failures() {
        assert_eq!(Fx::ONE.div_down(Fx::ZERO), Err(CoreError::DivByZero));
        assert_eq!(Fx::ZERO.checked_sub(Fx::ONE), Err(CoreError::Negative));
        assert!(Fx::from_raw(i128::MAX).unwrap().mul_down(fx("2")).is_err());
    }

    #[test]
    fn serde_uses_strings() {
        let v = fx("12.5");
        let j = serde_json::to_string(&v).unwrap();
        assert_eq!(j, "\"12.5000000\"");
        assert_eq!(serde_json::from_str::<Fx>(&j).unwrap(), v);
        assert!(serde_json::from_str::<Fx>("\"-1\"").is_err());
    }
}
