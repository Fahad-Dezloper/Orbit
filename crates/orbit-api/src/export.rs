//! Static export: the API's own responses written to JSON files.
//!
//! Each file is produced by sending the matching GET request to [`router`] in-process, so a
//! static host serves exactly what the live API would have served at export time, with no
//! second implementation to drift. Default query parameters only.
//!
//! | File | Request |
//! | --- | --- |
//! | `health.json` | `/health` |
//! | `v1/latest.json` | `/v1/latest` |
//! | `v1/lenders.json` | `/v1/lenders` |
//! | `v1/assets/{code}.json` | `/v1/assets/{code}` |
//! | `v1/assets/{code}/history.json` | `/v1/assets/{code}/history?band=1000&hours=24` |
//! | `v1/assets/{code}/cap.json` | `/v1/assets/{code}/cap` |
//! | `index.json` | when the export ran, and the status of each file |
//!
//! A static host cannot send an HTTP status, so a file may hold an error body such as
//! `{"error": "..."}` (for example when the latest run is stale). Readers must check for it
//! and must check `measured_at` themselves.

use std::fs;
use std::path::Path;

use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use crate::http::{now, router, Shared};

/// Writes every file for the given asset codes under `dir`. Returns the number of files.
pub async fn export(state: &Shared, dir: &Path, codes: &[String]) -> Result<usize, String> {
    let mut targets: Vec<(String, String)> = vec![
        ("/health".into(), "health.json".into()),
        ("/v1/latest".into(), "v1/latest.json".into()),
        ("/v1/lenders".into(), "v1/lenders.json".into()),
    ];
    for c in codes {
        if !c.bytes().all(|b| b.is_ascii_alphanumeric()) || c.is_empty() {
            return Err(format!("refusing to export unsafe asset code {c:?}"));
        }
        targets.push((format!("/v1/assets/{c}"), format!("v1/assets/{c}.json")));
        targets.push((
            format!("/v1/assets/{c}/history?band=1000&hours=24"),
            format!("v1/assets/{c}/history.json"),
        ));
        targets.push((
            format!("/v1/assets/{c}/cap"),
            format!("v1/assets/{c}/cap.json"),
        ));
    }
    let mut index = Vec::new();
    for (uri, file) in &targets {
        let req = Request::get(uri.as_str())
            .body(Body::empty())
            .map_err(|e| e.to_string())?;
        // The router cannot fail: its error type is `Infallible`.
        let resp = match router(state.clone()).oneshot(req).await {
            Ok(r) => r,
            Err(never) => match never {},
        };
        let status = resp.status().as_u16();
        let bytes = resp
            .into_body()
            .collect()
            .await
            .map_err(|e| e.to_string())?
            .to_bytes();
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        index.push(json!({"file": file, "request": uri, "status": status}));
    }
    let body = json!({
        "exported_at": now(),
        "files": index,
        "note": "Static copies of orbit-api responses. A file may hold {\"error\": ...}; \
                 always check measured_at before using a figure.",
    });
    let path = dir.join("index.json");
    let text = serde_json::to_string_pretty(&body).map_err(|e| e.to_string())?;
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(targets.len() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{AppState, Status};
    use crate::store::Store;
    use orbit_core::{Thresholds, WindowPolicy};
    use std::sync::{Arc, RwLock};

    #[tokio::test]
    async fn writes_error_bodies_for_an_empty_store_and_rejects_bad_codes() {
        let dir = std::env::temp_dir().join(format!("orbit-export-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let state: Shared = Arc::new(AppState {
            store: Store::open(dir.join("store")).unwrap(),
            policy: WindowPolicy::default(),
            thresholds: Thresholds::default(),
            max_staleness_secs: 7200,
            status: RwLock::new(Status::default()),
        });
        let out = dir.join("site");
        let n = export(&state, &out, &["USTRY".into()]).await.unwrap();
        assert_eq!(n, 7);
        let latest = fs::read_to_string(out.join("v1/latest.json")).unwrap();
        assert!(latest.contains("error"));
        let index = fs::read_to_string(out.join("index.json")).unwrap();
        assert!(index.contains("\"status\": 503"));
        assert!(export(&state, &out, &["../x".into()]).await.is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
