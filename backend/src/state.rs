use sqlx::PgPool;

#[derive(Clone, Default)]
pub struct AppState {
    pub db: Option<PgPool>,
}

impl AppState {
    pub fn new(db: Option<PgPool>) -> Self {
        Self { db }
    }
}
