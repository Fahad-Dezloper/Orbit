//! Manipulation resistance: the published figure is the minimum over a time window.
//!
//! Depth that is placed right before a measurement and pulled right after can only ever
//! *lower* the published figure, because we publish the smallest value seen in the
//! window. Coverage rules make the window fail closed: too few samples, or a gap long
//! enough to hide a dip, and nothing is published.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;
use crate::fixed::Fx;

/// One measurement of an asset's total exit (USD) at a fixed band.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sample {
    /// Unix seconds.
    pub ts: u64,
    pub exit_usd: Fx,
}

/// Coverage requirements for a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowPolicy {
    /// Length of the window in seconds (default 24h).
    pub window_secs: u64,
    /// Minimum samples inside the window.
    pub min_samples: usize,
    /// Largest allowed gap between consecutive samples, and between the last sample and now.
    pub max_gap_secs: u64,
}

impl Default for WindowPolicy {
    fn default() -> Self {
        WindowPolicy {
            window_secs: 24 * 3600,
            min_samples: 12,
            max_gap_secs: 2 * 3600,
        }
    }
}

/// The windowed-minimum result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowedDepth {
    pub min_exit_usd: Fx,
    pub samples: usize,
    pub from_ts: u64,
    pub to_ts: u64,
}

/// Minimum exit over `[now - window, now]`, or an error if coverage is insufficient.
/// Samples may be given in any order; samples from the future are ignored.
pub fn windowed_min(
    samples: &[Sample],
    now: u64,
    p: &WindowPolicy,
) -> Result<WindowedDepth, CoreError> {
    let start = now.saturating_sub(p.window_secs);
    let mut inside: Vec<Sample> = samples
        .iter()
        .copied()
        .filter(|s| s.ts >= start && s.ts <= now)
        .collect();
    inside.sort_by_key(|s| s.ts);

    if inside.len() < p.min_samples || inside.is_empty() {
        return Err(CoreError::InsufficientSamples {
            have: inside.len(),
            need: p.min_samples.max(1),
        });
    }
    for w in inside.windows(2) {
        let gap = w[1].ts - w[0].ts;
        if gap > p.max_gap_secs {
            return Err(CoreError::SampleGap {
                gap_secs: gap,
                max_gap_secs: p.max_gap_secs,
            });
        }
    }
    let last = inside[inside.len() - 1].ts;
    if now - last > p.max_gap_secs {
        return Err(CoreError::SampleGap {
            gap_secs: now - last,
            max_gap_secs: p.max_gap_secs,
        });
    }
    let min = inside.iter().map(|s| s.exit_usd).min().unwrap_or(Fx::ZERO);
    Ok(WindowedDepth {
        min_exit_usd: min,
        samples: inside.len(),
        from_ts: inside[0].ts,
        to_ts: last,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(s: &str) -> Fx {
        s.parse().unwrap()
    }
    fn hourly(values: &[&str], end: u64) -> Vec<Sample> {
        let n = values.len() as u64;
        values
            .iter()
            .enumerate()
            .map(|(i, v)| Sample {
                ts: end - (n - 1 - i as u64) * 3600,
                exit_usd: fx(v),
            })
            .collect()
    }
    const NOW: u64 = 1_800_000_000;

    #[test]
    fn spoofed_spike_does_not_raise_the_figure() {
        let mut v = vec!["1000"; 24];
        v[23] = "5000000"; // depth parked right before the last measurement
        let d = windowed_min(&hourly(&v, NOW), NOW, &WindowPolicy::default()).unwrap();
        assert_eq!(d.min_exit_usd, fx("1000"));
    }

    #[test]
    fn a_dip_anywhere_in_the_window_is_published() {
        let mut v = vec!["268000"; 24];
        v[5] = "0"; // the book was drained for an hour, as USTRY was in February
        let d = windowed_min(&hourly(&v, NOW), NOW, &WindowPolicy::default()).unwrap();
        assert_eq!(d.min_exit_usd, Fx::ZERO);
    }

    #[test]
    fn fails_closed_on_thin_coverage() {
        let v = vec!["1000"; 5];
        assert!(matches!(
            windowed_min(&hourly(&v, NOW), NOW, &WindowPolicy::default()),
            Err(CoreError::InsufficientSamples { have: 5, need: 12 })
        ));
    }

    #[test]
    fn fails_closed_on_gaps_and_staleness() {
        let mut s = hourly(&vec!["1000"; 24], NOW);
        s.remove(10);
        s.remove(10);
        s.remove(10); // a 4h hole
        assert!(matches!(
            windowed_min(&s, NOW, &WindowPolicy::default()),
            Err(CoreError::SampleGap { .. })
        ));

        let stale = hourly(&vec!["1000"; 20], NOW - 3 * 3600);
        assert!(matches!(
            windowed_min(&stale, NOW, &WindowPolicy::default()),
            Err(CoreError::SampleGap { .. })
        ));
    }

    #[test]
    fn ignores_samples_outside_the_window() {
        let mut s = hourly(&vec!["1000"; 24], NOW);
        s.push(Sample {
            ts: NOW - 30 * 3600,
            exit_usd: Fx::ZERO,
        }); // too old
        s.push(Sample {
            ts: NOW + 60,
            exit_usd: Fx::ZERO,
        }); // future
        assert_eq!(
            windowed_min(&s, NOW, &WindowPolicy::default())
                .unwrap()
                .min_exit_usd,
            fx("1000")
        );
    }
}
