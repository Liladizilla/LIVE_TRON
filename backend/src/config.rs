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
                secret.as_bytes().len() >= 32,
                "LIVE_TRON_JWT_SECRET must contain at least 32 bytes"
            );
        }
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
        })
    }
}
