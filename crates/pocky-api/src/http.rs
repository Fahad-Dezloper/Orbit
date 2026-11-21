//! Read-only HTTP API over the snapshot store.
//!
//! | Route | Returns |
//! | --- | --- |
//! | `GET /health` | service status, last run time, last error |
//! | `GET /v1/latest` | the latest run (every asset, every venue) |
//! | `GET /v1/assets/{code}` | latest report for one asset plus its windowed minimum per band |
//! | `GET /v1/assets/{code}/history?band=1000&hours=24` | exit series for one band |
//! | `GET /v1/assets/{code}/cap?bonus_bps=500&safety_bps=5000` | liquidatable collateral at a liquidation bonus, and a recommended cap |
//!
//! Fails closed: if the latest run is older than `max_staleness_secs`, `/v1/latest` returns
//! 503 instead of serving an old figure as if it were current, and a window without enough
//! coverage returns the reason instead of a number.

use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use pocky_core::{
    liquidatable_at, safe_cap, windowed_min, Band, Fx, Sample, Thresholds, Verdict, WindowPolicy,
};
use pocky_ingest::run::Run;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};

use crate::store::{samples, Store};

/// Longest history a single request may ask for.
pub const MAX_HISTORY_HOURS: u64 = 7 * 24;

/// What the measurement loop reports about itself.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub last_run_at: Option<u64>,
    pub last_error: Option<String>,
    pub runs_ok: u64,
    pub runs_failed: u64,
}

pub struct AppState {
    pub store: Store,
    pub policy: WindowPolicy,
    pub thresholds: Thresholds,
    pub max_staleness_secs: u64,
    pub status: RwLock<Status>,
}

pub type Shared = Arc<AppState>;

pub fn router(state: Shared) -> Router {
    // The data is public and every route is a GET, so any origin may read it.
    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any);
    Router::new()
        .route("/health", get(health))
        .route("/v1/latest", get(latest))
        .route("/v1/assets/{code}", get(asset))
        .route("/v1/assets/{code}/history", get(history))
        .route("/v1/assets/{code}/cap", get(cap))
        .layer(cors)
        .with_state(state)
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn error(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({ "error": msg.into() }))).into_response()
}

fn internal(e: impl std::fmt::Display) -> Response {
    error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn valid_code(code: &str) -> bool {
    (1..=12).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphanumeric())
}

async fn health(State(s): State<Shared>) -> Response {
    let status = match s.status.read() {
        Ok(g) => g.clone(),
        Err(_) => return internal("status lock poisoned"),
    };
    Json(json!({ "ok": true, "now": now(), "status": status })).into_response()
}

fn latest_run(s: &AppState) -> Result<Run, Box<Response>> {
    match s.store.latest(now(), s.max_staleness_secs) {
        Ok(Some(r)) => Ok(r),
        Ok(None) => Err(Box::new(error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("no measurement in the last {}s", s.max_staleness_secs),
        ))),
        Err(e) => Err(Box::new(internal(e))),
    }
}

async fn latest(State(s): State<Shared>) -> Response {
    match latest_run(&s) {
        Ok(r) => Json(r).into_response(),
        Err(resp) => *resp,
    }
}

/// The windowed minimum at one band, or why there is none.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Windowed {
    Ok {
        band: Band,
        min_exit_usd: Fx,
        verdict: Verdict,
        samples: usize,
        from_ts: u64,
        to_ts: u64,
    },
    Insufficient {
        band: Band,
        reason: String,
    },
}

pub fn windowed(points: &[Sample], band: Band, at: u64, s: &AppState) -> Windowed {
    match windowed_min(points, at, &s.policy) {
        Ok(w) => Windowed::Ok {
            band,
            min_exit_usd: w.min_exit_usd,
            verdict: s.thresholds.classify(w.min_exit_usd),
            samples: w.samples,
            from_ts: w.from_ts,
            to_ts: w.to_ts,
        },
        Err(e) => Windowed::Insufficient {
            band,
            reason: e.to_string(),
        },
    }
}

async fn asset(State(s): State<Shared>, Path(code): Path<String>) -> Response {
    if !valid_code(&code) {
        return error(StatusCode::BAD_REQUEST, "invalid asset code");
    }
    let run = match latest_run(&s) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    let Some(report) = run.reports.into_iter().find(|r| r.code == code) else {
        return error(StatusCode::NOT_FOUND, format!("{code} is not measured"));
    };
    let at = now();
    let loaded = match s.store.load(at.saturating_sub(s.policy.window_secs), at) {
        Ok(l) => l,
        Err(e) => return internal(e),
    };
    let windows: Vec<Windowed> = Band::STANDARD
        .iter()
        .map(|&b| windowed(&samples(&loaded.runs, &code, b), b, at, &s))
        .collect();
    Json(json!({
        "measured_at": run.measured_at,
        "report": report,
        "window_secs": s.policy.window_secs,
        "windowed": windows,
    }))
    .into_response()
}

#[derive(Deserialize)]
struct HistoryQuery {
    band: Option<u32>,
    hours: Option<u64>,
}

async fn history(
    State(s): State<Shared>,
    Path(code): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Response {
    if !valid_code(&code) {
        return error(StatusCode::BAD_REQUEST, "invalid asset code");
    }
    let band = match Band::new(q.band.unwrap_or(1_000)) {
        Ok(b) => b,
        Err(e) => return error(StatusCode::BAD_REQUEST, e.to_string()),
    };
    let hours = q.hours.unwrap_or(24).clamp(1, MAX_HISTORY_HOURS);
    let at = now();
    let loaded = match s.store.load(at.saturating_sub(hours * 3600), at) {
        Ok(l) => l,
        Err(e) => return internal(e),
    };
    Json(json!({
        "code": code,
        "band": band,
        "hours": hours,
        "points": samples(&loaded.runs, &code, band),
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use pocky_core::AssetDepth;
    use pocky_ingest::AssetReport;
    use tower::ServiceExt;

    fn state(name: &str) -> Shared {
        let dir = std::env::temp_dir().join(format!("pocky-http-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Arc::new(AppState {
            store: Store::open(dir).unwrap(),
            policy: WindowPolicy::default(),
            thresholds: Thresholds::default(),
            max_staleness_secs: 7200,
            status: RwLock::new(Status::default()),
        })
    }

    fn run(ts: u64, usd: &str) -> Run {
        let t = Thresholds::default();
        let depths = Band::STANDARD
            .iter()
            .map(|&b| AssetDepth {
                total_usd: usd.parse().unwrap(),
                ..AssetDepth::combine("USTRY", b, vec![], &t).unwrap()
            })
            .collect();
        Run {
            measured_at: ts,
            xlm_usd: "0.125".into(),
            venues: "test".into(),
            reports: vec![AssetReport {
                code: "USTRY".into(),
                issuer: "G".into(),
                reference: None,
                venue_mids: vec![],
                depths,
                notes: vec![],
                venues_read: 0,
                curve: vec![],
            }],
            skipped: vec![],
            stats: Default::default(),
        }
    }

    async fn get(s: &Shared, uri: &str) -> (StatusCode, serde_json::Value) {
        let resp = router(s.clone())
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let code = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (code, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn empty_store_fails_closed() {
        let s = state("empty");
        let (code, body) = get(&s, "/v1/latest").await;
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body["error"].as_str().unwrap().contains("no measurement"));
        assert_eq!(get(&s, "/health").await.0, StatusCode::OK);
    }

    #[tokio::test]
    async fn window_needs_coverage_then_reports_the_minimum() {
        let s = state("window");
        let t = now();
        s.store.append(&run(t, "300000")).unwrap();
        let (code, body) = get(&s, "/v1/assets/USTRY").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["windowed"][1]["status"], "insufficient");

        // 13 hourly runs, one of them a dip: the window reports the dip.
        for i in 1..13u64 {
            let v = if i == 5 { "40000" } else { "300000" };
            s.store.append(&run(t - i * 3600, v)).unwrap();
        }
        let (_, body) = get(&s, "/v1/assets/USTRY").await;
        let w = &body["windowed"][1];
        assert_eq!(w["status"], "ok");
        assert_eq!(w["min_exit_usd"], "40000.0000000");
        assert_eq!(w["verdict"], "shallow");
        assert_eq!(w["samples"], 13);

        let (code, body) = get(&s, "/v1/assets/USTRY/history?band=1000&hours=24").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["points"].as_array().unwrap().len(), 13);
    }

    #[tokio::test]
    async fn cap_needs_a_window_and_applies_the_margin() {
        let s = state("cap");
        let t = now();
        let with_curve = |ts: u64, usd: &str| {
            let mut r = run(ts, usd);
            r.reports[0].curve = vec![pocky_core::CurvePoint {
                band: Band::new(500).unwrap(),
                exit_usd: usd.parse().unwrap(),
            }];
            r
        };
        s.store.append(&with_curve(t, "200000")).unwrap();
        let (code, body) = get(&s, "/v1/assets/USTRY/cap?bonus_bps=500").await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["liquidatable_now_usd"], "200000.0000000");
        assert!(
            body["recommended_cap_usd"].is_null(),
            "no cap without a full window"
        );

        for i in 1..13u64 {
            let v = if i == 4 { "80000" } else { "200000" };
            s.store.append(&with_curve(t - i * 3600, v)).unwrap();
        }
        let (_, body) = get(&s, "/v1/assets/USTRY/cap?bonus_bps=500&safety_bps=5000").await;
        assert_eq!(body["window"]["min_usd"], "80000.0000000");
        assert_eq!(body["recommended_cap_usd"], "40000.0000000");
        // A bonus below the smallest sampled band claims nothing.
        let (_, body) = get(&s, "/v1/assets/USTRY/cap?bonus_bps=10").await;
        assert!(body["liquidatable_now_usd"].is_null());
        assert_eq!(
            get(&s, "/v1/assets/USTRY/cap?bonus_bps=0").await.0,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn rejects_bad_input() {
        let s = state("bad");
        s.store.append(&run(now(), "1")).unwrap();
        assert_eq!(
            get(&s, "/v1/assets/US%20TRY").await.0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(get(&s, "/v1/assets/KTB").await.0, StatusCode::NOT_FOUND);
        let (code, _) = get(&s, "/v1/assets/USTRY/history?band=0").await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
    }
}

#[derive(Deserialize)]
struct CapQuery {
    bonus_bps: Option<u32>,
    safety_bps: Option<u32>,
}

/// Liquidatable collateral at a liquidation bonus, now and as the 24h minimum, and a
/// recommended cap. The cap is built from the windowed minimum only: if the window lacks
/// coverage there is no recommendation.
async fn cap(
    State(s): State<Shared>,
    Path(code): Path<String>,
    Query(q): Query<CapQuery>,
) -> Response {
    if !valid_code(&code) {
        return error(StatusCode::BAD_REQUEST, "invalid asset code");
    }
    let bonus = q.bonus_bps.unwrap_or(500);
    let safety = q.safety_bps.unwrap_or(5_000);
    if bonus == 0 || bonus >= 10_000 || safety >= 10_000 {
        return error(
            StatusCode::BAD_REQUEST,
            "bonus_bps must be 1..9999 and safety_bps 0..9999",
        );
    }
    let run = match latest_run(&s) {
        Ok(r) => r,
        Err(resp) => return *resp,
    };
    let Some(report) = run.reports.iter().find(|r| r.code == code) else {
        return error(StatusCode::NOT_FOUND, format!("{code} is not measured"));
    };
    let now_usd = liquidatable_at(&report.curve, bonus);
    let at = now();
    let loaded = match s.store.load(at.saturating_sub(s.policy.window_secs), at) {
        Ok(l) => l,
        Err(e) => return internal(e),
    };
    let points: Vec<Sample> = loaded
        .runs
        .iter()
        .filter_map(|r| {
            let rep = r.reports.iter().find(|x| x.code == code)?;
            Some(Sample {
                ts: r.measured_at,
                exit_usd: liquidatable_at(&rep.curve, bonus)?,
            })
        })
        .collect();
    let (window, recommended) = match windowed_min(&points, at, &s.policy) {
        Ok(w) => match safe_cap(w.min_exit_usd, safety) {
            Ok(c) => (
                json!({"status": "ok", "min_usd": w.min_exit_usd, "samples": w.samples}),
                Some(c),
            ),
            Err(e) => return internal(e),
        },
        Err(e) => (
            json!({"status": "insufficient", "reason": e.to_string()}),
            None,
        ),
    };
    Json(json!({
        "code": code,
        "bonus_bps": bonus,
        "safety_bps": safety,
        "measured_at": run.measured_at,
        "liquidatable_now_usd": now_usd,
        "window": window,
        "recommended_cap_usd": recommended,
        "meaning": "Collateral above liquidatable_usd cannot be liquidated on Stellar at this \
                    bonus without the lender taking the loss. The cap uses the 24h minimum, \
                    reduced by safety_bps.",
    }))
    .into_response()
}
