//! When to measure next.
//!
//! Measuring at fixed, predictable times would let someone place orders just before each
//! measurement and cancel them just after, paying only fees to inflate every sample. So each
//! wait is the interval scaled by a random factor between 0.75 and 1.25: on average the
//! cadence is unchanged, but nobody outside knows the next sample time to the minute.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Spread of the random factor around 1.0, in basis points (±25%).
pub const JITTER_BPS: u64 = 2_500;

/// SplitMix64: a small, well-mixed hash; enough to make timing unpredictable from outside.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// `interval` scaled by a factor in `[1 - JITTER, 1 + JITTER]` chosen from `seed`.
pub fn jittered(interval: Duration, seed: u64) -> Duration {
    let span = 2 * JITTER_BPS + 1;
    let bps = 10_000 - JITTER_BPS + mix(seed) % span; // 7_500 ..= 12_500
    let ms = u64::try_from(interval.as_millis()).unwrap_or(u64::MAX);
    Duration::from_millis(ms / 10_000 * bps)
}

/// A seed that differs on every call.
pub fn seed() -> u64 {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    (n as u64) ^ u64::from(std::process::id()).rotate_left(32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_within_a_quarter_of_the_interval_and_varies() {
        let base = Duration::from_secs(1800);
        let mut seen = std::collections::BTreeSet::new();
        for s in 0..1_000u64 {
            let d = jittered(base, s);
            assert!(
                d >= Duration::from_secs(1350) && d <= Duration::from_secs(2250),
                "{d:?}"
            );
            seen.insert(d.as_secs() / 60);
        }
        // Spread across most of the 22.5 to 37.5 minute range.
        assert!(seen.len() > 10, "{seen:?}");
    }
}
