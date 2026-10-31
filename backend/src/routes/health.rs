use crate::state::AppState;
use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

pub async fn healthz() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "live-tron-api",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

pub async fn readyz(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> impl IntoResponse {
    let Some(pool) = state.db else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "not_ready",
                "checks": { "database": "not_configured" }
            })),
        );
    };

    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&pool)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "status": "ready",
                "checks": { "database": "ok" }
            })),
        ),
        Err(error) => {
            tracing::warn!(error = %error, "database readiness check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "status": "not_ready",
                    "checks": { "database": "unavailable" }
                })),
            )
        }
    }
}
