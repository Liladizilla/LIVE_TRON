pub mod auth;
pub mod channels;
mod health;
mod media_auth;
mod meta;
pub mod signaling;
pub mod streams;

use crate::state::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route("/v1", get(meta::api_info))
        .route(
            "/internal/v1/media/authorize",
            post(media_auth::authorize_media),
        )
        .route("/v1/auth/register", post(auth::register))
        .route("/v1/auth/login", post(auth::login))
        .route("/v1/auth/refresh", post(auth::refresh))
        .route("/v1/auth/logout", post(auth::logout))
        .route("/v1/me", get(auth::me).patch(auth::update_me))
        .route(
            "/v1/channels",
            get(channels::discover).post(channels::create),
        )
        .route(
            "/v1/channels/{slug}",
            get(channels::get_public).patch(channels::update),
        )
        .route(
            "/v1/channels/{slug}/follow",
            post(channels::follow).delete(channels::unfollow),
        )
        .route("/v1/channels/{slug}/streams", post(streams::create))
        .route("/v1/streams", get(streams::discover))
        .route("/v1/media/ice-config", get(signaling::ice_config))
        .route(
            "/v1/streams/{stream_id}/signal-ticket",
            post(signaling::create_ticket),
        )
        .route("/v1/streams/{stream_id}/signal", get(signaling::connect))
        .route(
            "/internal/v1/streams/{stream_id}/authorize-publish",
            post(streams::authorize_publish),
        )
        .route(
            "/v1/streams/{stream_id}",
            get(streams::get_public).patch(streams::update),
        )
        .route(
            "/v1/streams/{stream_id}/publishing-key/rotate",
            post(streams::rotate_key),
        )
        .route(
            "/v1/streams/{stream_id}/publishing-key",
            axum::routing::delete(streams::revoke_key),
        )
        .route("/internal/v1/media/events", post(streams::media_event))
        .route("/v1/creators/{slug}", get(channels::creator_profile))
}
