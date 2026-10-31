use axum::Json;
use serde_json::json;

pub async fn api_info() -> Json<serde_json::Value> {
    Json(json!({
        "name": "LIVE_TRON API",
        "version": env!("CARGO_PKG_VERSION"),
        "status": "foundation",
        "media_plane": "not_connected"
    }))
}
