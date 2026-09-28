//! Money is stored as integer micro-USD. No floats in the ledger (plan section 9).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Sub};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MicroUsd(pub i64);

impl MicroUsd {
    pub const ZERO: MicroUsd = MicroUsd(0);

    /// Build from a dollar amount given as a string like "0.40" (exact, no float parsing).
    pub fn parse_dollars(s: &str) -> Option<MicroUsd> {
        let s = s.trim();
        let (neg, s) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s),
        };
        let (whole, frac) = match s.split_once('.') {
            Some((w, f)) => (w, f),
            None => (s, ""),
        };
        if whole.is_empty() && frac.is_empty() {
            return None;
        }
        if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) || frac.len() > 6 {
            return None;
        }
        let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
        let mut frac_s = frac.to_string();
        while frac_s.len() < 6 {
            frac_s.push('0');
        }
        let frac: i64 = frac_s.parse().ok()?;
        let v = whole.checked_mul(1_000_000)?.checked_add(frac)?;
        Some(MicroUsd(if neg { -v } else { v }))
    }

    /// Price per one million tokens times a token count, rounded up (conservative).
    pub fn for_tokens(price_per_million: MicroUsd, tokens: u64) -> MicroUsd {
        let total = (price_per_million.0 as i128) * (tokens as i128);
        MicroUsd(((total + 999_999) / 1_000_000) as i64)
    }

    pub fn saturating_add(self, o: MicroUsd) -> MicroUsd {
        MicroUsd(self.0.saturating_add(o.0))
    }
}

impl Add for MicroUsd {
    type Output = MicroUsd;
    fn add(self, o: MicroUsd) -> MicroUsd {
        MicroUsd(self.0 + o.0)
    }
}
impl AddAssign for MicroUsd {
    fn add_assign(&mut self, o: MicroUsd) {
        self.0 += o.0;
    }
}
impl Sub for MicroUsd {
    type Output = MicroUsd;
    fn sub(self, o: MicroUsd) -> MicroUsd {
        MicroUsd(self.0 - o.0)
    }
}
impl fmt::Display for MicroUsd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let v = self.0.unsigned_abs();
        write!(f, "{sign}${}.{:06}", v / 1_000_000, v % 1_000_000)
    }
}
impl fmt::Debug for MicroUsd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dollars_exactly() {
        assert_eq!(MicroUsd::parse_dollars("0.40"), Some(MicroUsd(400_000)));
        assert_eq!(MicroUsd::parse_dollars("2"), Some(MicroUsd(2_000_000)));
        assert_eq!(MicroUsd::parse_dollars(".5"), Some(MicroUsd(500_000)));
        assert_eq!(MicroUsd::parse_dollars("0.000001"), Some(MicroUsd(1)));
        assert_eq!(MicroUsd::parse_dollars("0.0000001"), None);
        assert_eq!(MicroUsd::parse_dollars("abc"), None);
        assert_eq!(MicroUsd::parse_dollars(""), None);
    }

    #[test]
    fn token_pricing_rounds_up() {
        let p = MicroUsd::parse_dollars("3.00").unwrap(); // $3 per 1M tokens
        assert_eq!(MicroUsd::for_tokens(p, 1_000_000), MicroUsd(3_000_000));
        assert_eq!(MicroUsd::for_tokens(p, 1), MicroUsd(3));
        assert_eq!(MicroUsd::for_tokens(p, 0), MicroUsd::ZERO);
    }

    #[test]
    fn displays_as_dollars() {
        assert_eq!(MicroUsd(1_234_567).to_string(), "$1.234567");
        assert_eq!(MicroUsd(-5).to_string(), "-$0.000005");
    }
}
