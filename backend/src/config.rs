use anyhow::{Context, Result};
use axum::http::HeaderValue;
use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct Settings {
    pub bind_addr: SocketAddr,
    pub database_url: Option<String>,
    pub cors_origins: Vec<HeaderValue>,
    pub database_max_connections: u32,
    pub jwt_secret: Option<String>,
    pub media_event_secret: Option<String>,
    pub mediamtx_auth_secret: Option<String>,
    pub stun_urls: Vec<String>,
    pub turn_urls: Vec<String>,
    pub turn_shared_secret: Option<String>,
}

fn csv_env(name: &str, default: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_else(|_| default.to_owned())
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

impl Settings {
    pub fn from_env() -> Result<Self> {
        let bind_addr = std::env::var("LIVE_TRON_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_owned())
            .parse()
            .context("LIVE_TRON_BIND must be a valid socket address")?;
        let database_url = std::env::var("DATABASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let database_max_connections = std::env::var("LIVE_TRON_DATABASE_MAX_CONNECTIONS")
            .unwrap_or_else(|_| "10".to_owned())
            .parse::<u32>()
            .context("LIVE_TRON_DATABASE_MAX_CONNECTIONS must be an integer")?;
        anyhow::ensure!(
            (1..=100).contains(&database_max_connections),
            "LIVE_TRON_DATABASE_MAX_CONNECTIONS must be between 1 and 100"
        );
        let jwt_secret = std::env::var("LIVE_TRON_JWT_SECRET")
            .ok()
            .filter(|value| !value.trim().is_empty());
        if let Some(secret) = &jwt_secret {
            anyhow::ensure!(
                secret.len() >= 32,
                "LIVE_TRON_JWT_SECRET must contain at least 32 bytes"
            );
        }
        let media_event_secret = std::env::var("LIVE_TRON_MEDIA_EVENT_SECRET")
            .ok()
            .filter(|value| !value.trim().is_empty());
        if let Some(secret) = &media_event_secret {
            anyhow::ensure!(
                secret.len() >= 32,
                "LIVE_TRON_MEDIA_EVENT_SECRET must contain at least 32 bytes"
            );
        }
        let mediamtx_auth_secret = std::env::var("LIVE_TRON_MEDIAMTX_AUTH_SECRET")
            .ok()
            .filter(|value| !value.trim().is_empty());
        if let Some(secret) = &mediamtx_auth_secret {
            anyhow::ensure!(
                secret.len() >= 32,
                "LIVE_TRON_MEDIAMTX_AUTH_SECRET must contain at least 32 bytes"
            );
        }
        let stun_urls = csv_env("LIVE_TRON_STUN_URLS", "stun:stun.l.google.com:19302");
        anyhow::ensure!(
            stun_urls
                .iter()
                .all(|url| url.starts_with("stun:") || url.starts_with("stuns:")),
            "LIVE_TRON_STUN_URLS must contain only stun: or stuns: URLs"
        );
        let turn_urls = csv_env("LIVE_TRON_TURN_URLS", "");
        anyhow::ensure!(
            turn_urls
                .iter()
                .all(|url| url.starts_with("turn:") || url.starts_with("turns:")),
            "LIVE_TRON_TURN_URLS must contain only turn: or turns: URLs"
        );
        let turn_shared_secret = std::env::var("LIVE_TRON_TURN_SHARED_SECRET")
            .ok()
            .filter(|value| !value.trim().is_empty());
        if let Some(secret) = &turn_shared_secret {
            anyhow::ensure!(
                secret.len() >= 32,
                "LIVE_TRON_TURN_SHARED_SECRET must contain at least 32 bytes"
            );
        }
        anyhow::ensure!(
            turn_urls.is_empty() == turn_shared_secret.is_none(),
            "LIVE_TRON_TURN_URLS and LIVE_TRON_TURN_SHARED_SECRET must be configured together"
        );
        let cors_origins = std::env::var("LIVE_TRON_CORS_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(|origin| {
                origin
                    .parse::<HeaderValue>()
                    .with_context(|| format!("Invalid CORS origin: {origin}"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            bind_addr,
            database_url,
            cors_origins,
            database_max_connections,
            jwt_secret,
            media_event_secret,
            mediamtx_auth_secret,
            stun_urls,
            turn_urls,
            turn_shared_secret,
        })
    }
}
