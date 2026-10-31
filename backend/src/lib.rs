pub mod config;
pub mod routes;
pub mod state;

use axum::Router;
use state::AppState;
use tower_http::cors::{AllowOrigin, CorsLayer};
use axum::http::{header, HeaderValue, Method};

pub fn build_app(state: AppState, allowed_origins: &[HeaderValue]) -> Router {
    let cors = if allowed_origins.is_empty() {
        CorsLayer::new()
    } else {
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(allowed_origins.to_vec()))
            .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE, Method::OPTIONS])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
    };

    Router::new()
        .merge(routes::router())
        .with_state(state)
        .layer(cors)
        .layer(tower_http::trace::TraceLayer::new_for_http())
}
