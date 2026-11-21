//! Append-only snapshot store.
//!
//! Every measurement run is appended as one JSON line to a file per UTC day
//! (`snapshots-YYYY-MM-DD.jsonl`). Nothing is ever rewritten, so the history behind any
//! published figure stays inspectable. A line that cannot be parsed (for example a write cut
//! short by a crash) is skipped and counted, never guessed at.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use pocky_core::{Band, Sample};
use pocky_ingest::run::Run;
use thiserror::Error;

const DAY: u64 = 86_400;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("snapshot store at {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("cannot encode snapshot: {0}")]
    Encode(#[from] serde_json::Error),
}

fn io(path: &Path, source: std::io::Error) -> StoreError {
    StoreError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// `(year, month, day)` of a count of days since 1970-01-01 (proleptic Gregorian, UTC).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Howard Hinnant's algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

fn file_name(ts: u64) -> String {
    let (y, m, d) = civil_from_days((ts / DAY) as i64);
    format!("snapshots-{y:04}-{m:02}-{d:02}.jsonl")
}

/// What a load found, including lines it had to skip.
#[derive(Debug, Default)]
pub struct Loaded {
    pub runs: Vec<Run>,
    pub skipped_lines: usize,
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Store, StoreError> {
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        Ok(Store { dir })
    }

    /// Appends one run as one line and flushes it to disk.
    pub fn append(&self, run: &Run) -> Result<(), StoreError> {
        let path = self.dir.join(file_name(run.measured_at));
        let mut line = serde_json::to_vec(run)?;
        line.push(b'\n');
        let mut f = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)
            .map_err(|e| io(&path, e))?;
        // If an earlier write was cut short, start on a fresh line so this run is not
        // merged into the torn one.
        let len = f.metadata().map_err(|e| io(&path, e))?.len();
        if len > 0 {
            let mut last = [0u8; 1];
            f.seek(SeekFrom::Start(len - 1)).map_err(|e| io(&path, e))?;
            f.read_exact(&mut last).map_err(|e| io(&path, e))?;
            if last[0] != b'\n' {
                line.insert(0, b'\n');
            }
        }
        f.write_all(&line).map_err(|e| io(&path, e))?;
        f.sync_data().map_err(|e| io(&path, e))
    }

    /// Every run measured in `[from, to]`, oldest first.
    pub fn load(&self, from: u64, to: u64) -> Result<Loaded, StoreError> {
        let mut out = Loaded::default();
        let mut day = from / DAY;
        while day <= to / DAY {
            let path = self.dir.join(file_name(day * DAY));
            day += 1;
            let f = match File::open(&path) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io(&path, e)),
            };
            for line in BufReader::new(f).lines() {
                let line = line.map_err(|e| io(&path, e))?;
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Run>(&line) {
                    Ok(r) if r.measured_at >= from && r.measured_at <= to => out.runs.push(r),
                    Ok(_) => {}
                    Err(_) => out.skipped_lines += 1,
                }
            }
        }
        out.runs.sort_by_key(|r| r.measured_at);
        Ok(out)
    }

    /// The most recent run in the last `lookback_secs` before `now`.
    pub fn latest(&self, now: u64, lookback_secs: u64) -> Result<Option<Run>, StoreError> {
        Ok(self
            .load(now.saturating_sub(lookback_secs), now)?
            .runs
            .pop())
    }
}

/// Total exit for one asset at one band in every run where it was measured.
pub fn samples(runs: &[Run], code: &str, band: Band) -> Vec<Sample> {
    runs.iter()
        .filter_map(|run| {
            let report = run.reports.iter().find(|r| r.code == code)?;
            let depth = report.depths.iter().find(|d| d.band == band)?;
            Some(Sample {
                ts: run.measured_at,
                exit_usd: depth.total_usd,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocky_core::{AssetDepth, Fx, Thresholds};
    use pocky_ingest::AssetReport;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pocky-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn run(ts: u64, usd: &str) -> Run {
        let t = Thresholds::default();
        let depth = AssetDepth::combine("USTRY", Band::TEN_PCT, vec![], &t).unwrap();
        let depth = AssetDepth {
            total_usd: usd.parse::<Fx>().unwrap(),
            ..depth
        };
        Run {
            measured_at: ts,
            xlm_usd: "0.125".into(),
            venues: "test".into(),
            reports: vec![AssetReport {
                code: "USTRY".into(),
                issuer: "G".into(),
                reference: None,
                venue_mids: vec![],
                depths: vec![depth],
                notes: vec![],
            }],
            skipped: vec![],
        }
    }

    #[test]
    fn dates_files_by_utc_day() {
        assert_eq!(file_name(0), "snapshots-1970-01-01.jsonl");
        // 2026-11-21 12:00:00 UTC
        assert_eq!(file_name(1_795_262_400), "snapshots-2026-11-21.jsonl");
        // 2024-02-29 (leap day) 00:00:00 UTC
        assert_eq!(file_name(1_709_164_800), "snapshots-2024-02-29.jsonl");
    }

    #[test]
    fn appends_and_reloads_across_days() {
        let dir = temp_dir("reload");
        let s = Store::open(&dir).unwrap();
        let t0 = 1_795_262_400; // midday
        s.append(&run(t0, "100")).unwrap();
        s.append(&run(t0 + 13 * 3600, "90")).unwrap(); // next UTC day
        s.append(&run(t0 + 14 * 3600, "80")).unwrap();
        let all = s.load(t0, t0 + 14 * 3600).unwrap();
        assert_eq!(all.runs.len(), 3);
        assert_eq!(all.skipped_lines, 0);
        let pts = samples(&all.runs, "USTRY", Band::TEN_PCT);
        assert_eq!(pts.len(), 3);
        assert_eq!(pts[2].exit_usd, "80".parse().unwrap());
        assert!(samples(&all.runs, "USTRY", Band::TWO_PCT).is_empty());
        let latest = s.latest(t0 + 15 * 3600, 6 * 3600).unwrap().unwrap();
        assert_eq!(latest.measured_at, t0 + 14 * 3600);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_torn_line_is_skipped_not_guessed() {
        let dir = temp_dir("torn");
        let s = Store::open(&dir).unwrap();
        let t0 = 1_795_262_400;
        s.append(&run(t0, "100")).unwrap();
        let path = dir.join(file_name(t0));
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{\"measured_at\": 17952").unwrap(); // crash mid-write
        drop(f);
        s.append(&run(t0 + 60, "70")).unwrap();
        let all = s.load(t0, t0 + 60).unwrap();
        assert_eq!(all.runs.len(), 2, "the run after a torn write survives");
        assert_eq!(all.skipped_lines, 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
