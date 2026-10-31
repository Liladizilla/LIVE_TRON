pub mod auth;
pub mod channels;
mod health;
mod meta;

use crate::state::AppState;
use axum::{routing::{get, post}, Router};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/v1", get(meta::api_info))
        .route("/v1/auth/register", post(auth::register))
        .route("/v1/auth/login", post(auth::login))
        .route("/v1/auth/refresh", post(auth::refresh))
        .route("/v1/auth/logout", post(auth::logout))
        .route("/v1/me", get(auth::me).patch(auth::update_me))
        .route("/v1/channels", get(channels::discover).post(channels::create))
        .route("/v1/channels/:slug", get(channels::get_public).patch(channels::update))
        .route("/v1/channels/:slug/follow", post(channels::follow).delete(channels::unfollow))
        .route("/v1/creators/:slug", get(channels::creator_profile))
}
