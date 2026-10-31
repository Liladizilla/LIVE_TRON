use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct AppState {
    pub db: Option<PgPool>,
    pub jwt_secret: Option<Arc<String>>,
    pub media_event_secret: Option<Arc<String>>,
}

impl AppState {
    pub fn new(db: Option<PgPool>) -> Self {
        Self { db, jwt_secret: None, media_event_secret: None }
    }

    pub fn with_jwt_secret(mut self, secret: Option<String>) -> Self {
        self.jwt_secret = secret.map(Arc::new);
        self
    }

    pub fn with_media_event_secret(mut self, secret: Option<String>) -> Self {
        self.media_event_secret = secret.map(Arc::new);
        self
    }
}
