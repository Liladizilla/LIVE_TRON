use sqlx::{
    migrate::Migrator,
    postgres::{PgListener, PgPoolOptions},
    PgPool,
};
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;

async fn test_pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must point to the PostgreSQL CI service");
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("connect to PostgreSQL");
    let migrations = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations");
    Migrator::new(migrations)
        .await
        .expect("load migrations")
        .run(&pool)
        .await
        .expect("apply migrations");
    pool
}

async fn create_stream_fixture(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let suffix = Uuid::new_v4().simple().to_string();
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, password_hash, display_name) VALUES ($1, 'integration-test-hash', 'Signal Test') RETURNING id",
    )
    .bind(format!("signal-{suffix}@example.test"))
    .fetch_one(pool)
    .await
    .expect("create test user");
    let channel_id: Uuid = sqlx::query_scalar(
        "INSERT INTO channels (owner_user_id, slug, title, category) VALUES ($1, $2, 'Signal Test Channel', 'gaming') RETURNING id",
    )
    .bind(user_id)
    .bind(format!("signal-{suffix}"))
    .fetch_one(pool)
    .await
    .expect("create test channel");
    let stream_id: Uuid = sqlx::query_scalar(
        "INSERT INTO streams (channel_id, title, category) VALUES ($1, 'Distributed Signal Test', 'gaming') RETURNING id",
    )
    .bind(channel_id)
    .fetch_one(pool)
    .await
    .expect("create test stream");
    (user_id, channel_id, stream_id)
}

async fn reserve_viewer(pool: PgPool, stream_id: Uuid, participant_id: Uuid) -> bool {
    let mut tx = pool.begin().await.expect("begin peer reservation");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(stream_id.to_string())
        .execute(&mut *tx)
        .await
        .expect("lock stream reservation");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM signal_peer_leases WHERE stream_id = $1 AND expires_at > now()",
    )
    .bind(stream_id)
    .fetch_one(&mut *tx)
    .await
    .expect("count active peers");
    if count >= 256 {
        tx.rollback().await.expect("rollback full-room reservation");
        return false;
    }
    sqlx::query(
        "INSERT INTO signal_peer_leases (participant_id, stream_id, publisher, expires_at) VALUES ($1, $2, false, now() + interval '45 seconds')",
    )
    .bind(participant_id)
    .bind(stream_id)
    .execute(&mut *tx)
    .await
    .expect("insert viewer lease");
    tx.commit().await.expect("commit viewer lease");
    true
}

#[tokio::test]
async fn postgres_signal_notifications_reach_independent_api_listeners() {
    let pool = test_pool().await;
    let mut instance_a = PgListener::connect_with(&pool)
        .await
        .expect("connect instance A listener");
    let mut instance_b = PgListener::connect_with(&pool)
        .await
        .expect("connect instance B listener");
    instance_a
        .listen("live_tron_signal")
        .await
        .expect("subscribe A");
    instance_b
        .listen("live_tron_signal")
        .await
        .expect("subscribe B");

    let payload = format!(
        r#"{{"stream_id":"{}","event":{{"type":"peer_joined","from":"{}","role":"viewer"}}}}"#,
        Uuid::new_v4(),
        Uuid::new_v4()
    );
    sqlx::query("SELECT pg_notify('live_tron_signal', $1)")
        .bind(&payload)
        .execute(&pool)
        .await
        .expect("publish signaling event");

    let received_a = tokio::time::timeout(Duration::from_secs(3), instance_a.recv())
    .await
    .expect("instance A notification timed out")
    .expect("instance A listener failed");
    let received_b = tokio::time::timeout(Duration::from_secs(3), instance_b.recv())
    .await
    .expect("instance B notification timed out")
    .expect("instance B listener failed");
    assert_eq!(received_a.payload(), payload);
    assert_eq!(received_b.payload(), payload);
}

#[tokio::test]
async fn peer_capacity_reservations_are_serialized_across_api_instances() {
    let pool = test_pool().await;
    let (user_id, channel_id, stream_id) = create_stream_fixture(&pool).await;

    for _ in 0..255 {
        let inserted = reserve_viewer(pool.clone(), stream_id, Uuid::new_v4()).await;
        assert!(inserted, "room should accept its first 255 peers");
    }

    // These represent two API replicas racing to reserve the final room slot.
    let first = tokio::spawn(reserve_viewer(pool.clone(), stream_id, Uuid::new_v4()));
    let second = tokio::spawn(reserve_viewer(pool.clone(), stream_id, Uuid::new_v4()));
    let outcomes = [first.await.unwrap(), second.await.unwrap()];
    assert_eq!(outcomes.iter().filter(|accepted| **accepted).count(), 1);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM signal_peer_leases WHERE stream_id = $1 AND expires_at > now()",
    )
    .bind(stream_id)
    .fetch_one(&pool)
    .await
    .expect("read final peer count");
    assert_eq!(
        count, 256,
        "the distributed room cap must never be exceeded"
    );

    sqlx::query("DELETE FROM streams WHERE id = $1")
        .bind(stream_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(channel_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn database_rejects_a_second_publisher_lease_for_the_same_stream() {
    let pool = test_pool().await;
    let (user_id, channel_id, stream_id) = create_stream_fixture(&pool).await;
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO signal_peer_leases (participant_id, stream_id, publisher, expires_at) VALUES ($1, $2, true, now() + interval '45 seconds')",
    )
    .bind(first)
    .bind(stream_id)
    .execute(&pool)
    .await
    .expect("first publisher lease should succeed");

    let duplicate = sqlx::query(
        "INSERT INTO signal_peer_leases (participant_id, stream_id, publisher, expires_at) VALUES ($1, $2, true, now() + interval '45 seconds')",
    )
    .bind(second)
    .bind(stream_id)
    .execute(&pool)
    .await;
    assert!(
        duplicate
            .as_ref()
            .err()
            .and_then(sqlx::Error::as_database_error)
            .and_then(|error| error.code())
            .is_some_and(|code| code == "23505"),
        "a second publisher must fail with a unique-constraint violation"
    );

    sqlx::query("DELETE FROM streams WHERE id = $1")
        .bind(stream_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM channels WHERE id = $1")
        .bind(channel_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&pool)
        .await
        .unwrap();
}
