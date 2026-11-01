use sqlx::PgPool;
use std::{collections::HashMap, sync::Arc, time::Instant};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

pub struct SignalTicket {
    pub stream_id: Uuid,
    pub publisher: bool,
    pub expires_at: Instant,
}

pub struct SignalRoom {
    pub sender: broadcast::Sender<String>,
    pub publisher: Option<Uuid>,
    pub participants: HashMap<Uuid, bool>,
}

#[derive(Clone, Default)]
pub struct IceSettings {
    pub stun_urls: Vec<String>,
    pub turn_urls: Vec<String>,
    pub turn_shared_secret: Option<Arc<String>>,
}

#[derive(Clone, Default)]
pub struct AppState {
    pub db: Option<PgPool>,
    pub jwt_secret: Option<Arc<String>>,
    pub media_event_secret: Option<Arc<String>>,
    pub ice: IceSettings,
    pub allowed_origins: Arc<Vec<String>>,
    pub(crate) signal_rooms: Arc<Mutex<HashMap<Uuid, SignalRoom>>>,
    pub(crate) signal_tickets: Arc<Mutex<HashMap<String, SignalTicket>>>,
}

impl AppState {
    pub fn new(db: Option<PgPool>) -> Self {
        Self {
            db,
            ..Self::default()
        }
    }

    pub fn with_jwt_secret(mut self, secret: Option<String>) -> Self {
        self.jwt_secret = secret.map(Arc::new);
        self
    }

    pub fn with_media_event_secret(mut self, secret: Option<String>) -> Self {
        self.media_event_secret = secret.map(Arc::new);
        self
    }

    pub fn with_ice_settings(
        mut self,
        stun_urls: Vec<String>,
        turn_urls: Vec<String>,
        turn_shared_secret: Option<String>,
    ) -> Self {
        self.ice = IceSettings {
            stun_urls,
            turn_urls,
            turn_shared_secret: turn_shared_secret.map(Arc::new),
        };
        self
    }

    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = Arc::new(origins);
        self
    }
}
