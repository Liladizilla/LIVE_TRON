use anyhow::Context;
use sqlx::{migrate::Migrator, postgres::PgPoolOptions};
use std::{path::Path, time::Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let database_url =
        std::env::var("DATABASE_URL").context("DATABASE_URL is required to run migrations")?;
    let migrations_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let migrator = Migrator::new(&migrations_path)
        .await
        .context("Could not load migration directory")?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&database_url)
        .await
        .context("Could not connect to PostgreSQL")?;

    migrator
        .run(&pool)
        .await
        .context("Database migration failed")?;
    println!("LIVE_TRON database migrations are up to date.");
    pool.close().await;
    Ok(())
}
