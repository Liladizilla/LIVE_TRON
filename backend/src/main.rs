use anyhow::Context;
use live_tron_api::{build_app, config::Settings, routes::chat, state::AppState};
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let settings = Settings::from_env()?;
    let db = if let Some(database_url) = settings.database_url.as_deref() {
        let pool = PgPoolOptions::new()
            .max_connections(settings.database_max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(database_url)
            .await
            .context("Could not connect to PostgreSQL")?;
        tracing::info!("PostgreSQL connection established");
        Some(pool)
    } else {
        tracing::warn!("DATABASE_URL is not configured; readiness will remain unavailable");
        None
    };
    if settings.jwt_secret.is_none() {
        tracing::warn!(
            "LIVE_TRON_JWT_SECRET is missing; authenticated routes will remain unavailable"
        );
    }
    if settings.media_event_secret.is_none() {
        tracing::warn!(
            "LIVE_TRON_MEDIA_EVENT_SECRET is missing; internal media events will fail closed"
        );
    }
    if settings.mediamtx_auth_secret.is_none() {
        tracing::warn!(
            "LIVE_TRON_MEDIAMTX_AUTH_SECRET is missing; MediaMTX authentication will fail closed"
        );
    }
    let app_state = AppState::new(db)
        .with_jwt_secret(settings.jwt_secret)
        .with_media_event_secret(settings.media_event_secret)
        .with_mediamtx_auth_secret(settings.mediamtx_auth_secret)
        .with_ice_settings(
            settings.stun_urls,
            settings.turn_urls,
            settings.turn_shared_secret,
        );
    chat::start_chat_listener(app_state.clone());
    let app = build_app(app_state, &settings.cors_origins);
    let listener = TcpListener::bind(settings.bind_addr)
        .await
        .with_context(|| format!("Could not bind to {}", settings.bind_addr))?;
    tracing::info!(address = %settings.bind_addr, "LIVE_TRON API listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server failed")?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {}, }
    tracing::info!("shutdown signal received");
}
