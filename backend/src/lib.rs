pub mod config;
pub mod routes;
pub mod state;

use axum::{
    extract::DefaultBodyLimit,
    http::{header, HeaderValue, Method},
    Router,
};
use state::AppState;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub fn build_app(state: AppState, allowed_origins: &[HeaderValue]) -> Router {
    let state = state.with_allowed_origins(
        allowed_origins
            .iter()
            .filter_map(|origin| origin.to_str().ok().map(str::to_owned))
            .collect(),
    );
    let cors = if allowed_origins.is_empty() {
        CorsLayer::new()
    } else {
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(allowed_origins.to_vec()))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
    };

    Router::new()
        .merge(routes::router())
        .with_state(state)
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(cors)
        .layer(
            tower_http::trace::TraceLayer::new_for_http().make_span_with(
                |request: &axum::http::Request<_>| {
                    // Query strings can contain the MediaMTX callback secret. Never
                    // include them in request spans or structured logs.
                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        path = %request.uri().path()
                    )
                },
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_endpoint_reports_process_liveness() {
        let app = build_app(AppState::default(), &[]);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["status"], "ok");
        assert_eq!(value["service"], "live-tron-api");
    }

    #[tokio::test]
    async fn readiness_fails_closed_without_database_configuration() {
        let app = build_app(AppState::default(), &[]);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["checks"]["database"], "not_configured");
    }

    #[tokio::test]
    async fn api_metadata_does_not_claim_media_is_online() {
        let app = build_app(AppState::default(), &[]);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["media_plane"], "not_connected");
    }

    #[tokio::test]
    async fn mediamtx_auth_callback_fails_closed_without_configured_secret() {
        let app = build_app(AppState::default(), &[]);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/v1/media/authorize?secret=anything")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(
                        r#"{"action":"publish","path":"00000000-0000-4000-8000-000000000001","token":"lt_pub_invalid"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn mediamtx_auth_callback_rejects_a_wrong_shared_secret() {
        let app = build_app(
            AppState::default()
                .with_mediamtx_auth_secret(Some("correct-secret-value-32-bytes-long".to_owned())),
            &[],
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/internal/v1/media/authorize?secret=wrong-secret-value")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(
                        r#"{"action":"publish","path":"00000000-0000-4000-8000-000000000001","token":"lt_pub_invalid"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn creator_channel_listing_requires_authentication() {
        let app = build_app(
            AppState::default()
                .with_jwt_secret(Some("test-jwt-secret-at-least-32-bytes-long".to_owned())),
            &[],
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/me/channels")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn creator_stream_listing_requires_authentication() {
        let app = build_app(
            AppState::default()
                .with_jwt_secret(Some("test-jwt-secret-at-least-32-bytes-long".to_owned())),
            &[],
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/channels/my-channel/streams")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn oversized_json_requests_are_rejected() {
        let app = build_app(AppState::default(), &[]);
        let payload = format!(r#"{{"email":"{}"}}"#, "a".repeat(65 * 1024));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(axum::body::Body::from(payload))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
