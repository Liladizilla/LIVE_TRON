use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    Router,
};
use live_tron_api::{build_app, state::AppState};
use serde_json::{json, Value};
use sqlx::{migrate::Migrator, postgres::PgPoolOptions};
use std::path::PathBuf;
use tower::ServiceExt;
use uuid::Uuid;

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    bearer: Option<&str>,
    media_secret: Option<&str>,
    payload: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(secret) = media_secret {
        builder = builder.header("x-live-tron-media-secret", secret);
    }
    let body = if let Some(value) = payload {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(value.to_string())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

async fn register(app: &Router, email: String, display_name: &str) -> String {
    let (status, tokens) = request(app, Method::POST, "/v1/auth/register", None, None,
        Some(json!({"email": email, "password": "a-secure-test-password-123", "display_name": display_name}))).await;
    assert_eq!(status, StatusCode::CREATED, "{tokens}");
    tokens["access_token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn stream_lifecycle_credentials_visibility_and_media_events_are_guarded() {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must point to PostgreSQL CI service");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .unwrap();
    let migrations = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations");
    Migrator::new(migrations)
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();

    let suffix = Uuid::new_v4().simple().to_string();
    let jwt_secret = "integration-test-secret-with-at-least-32-bytes";
    let media_secret = "integration-media-secret-with-at-least-32-bytes";
    let app = build_app(
        AppState::new(Some(pool.clone()))
            .with_jwt_secret(Some(jwt_secret.to_owned()))
            .with_media_event_secret(Some(media_secret.to_owned())),
        &[],
    );
    let owner = register(
        &app,
        format!("stream-owner-{suffix}@example.test"),
        "Stream Owner",
    )
    .await;
    let viewer = register(
        &app,
        format!("stream-viewer-{suffix}@example.test"),
        "Stream Viewer",
    )
    .await;
    let slug = format!("stream-{suffix}");

    let (status, channel) = request(
        &app,
        Method::POST,
        "/v1/channels",
        Some(&owner),
        None,
        Some(json!({"slug": slug, "title": "Stream Test Channel", "category": "gaming"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{channel}");

    // 1. Stream creation is authenticated and channel-owner scoped.
    let (status, created) = request(
        &app,
        Method::POST,
        &format!("/v1/channels/{slug}/streams"),
        Some(&owner),
        None,
        Some(json!({"title": "First Broadcast", "category": "gaming", "visibility": "public"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let stream_id = created["stream"]["id"].as_str().unwrap().to_owned();
    let first_key = created["publishing_key"].as_str().unwrap().to_owned();
    assert!(first_key.starts_with("lt_pub_"));
    assert_eq!(created["stream"]["status"], "created");
    assert!(created["publishing_key_notice"]
        .as_str()
        .unwrap()
        .contains("shown only once"));

    // Private stream details are not disclosed to viewers or anonymous callers.
    let (status, private_stream) = request(
        &app,
        Method::POST,
        &format!("/v1/channels/{slug}/streams"),
        Some(&owner),
        None,
        Some(json!({"title": "Private Broadcast", "visibility": "private"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let private_id = private_stream["stream"]["id"].as_str().unwrap().to_owned();
    let (status, _) = request(
        &app,
        Method::GET,
        &format!("/v1/streams/{private_id}"),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = request(
        &app,
        Method::GET,
        &format!("/v1/streams/{private_id}"),
        Some(&viewer),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, private_read) = request(
        &app,
        Method::GET,
        &format!("/v1/streams/{private_id}"),
        Some(&owner),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(private_read["visibility"], "private");

    // A different account cannot create streams on the channel.
    let (status, _) = request(
        &app,
        Method::POST,
        &format!("/v1/channels/{slug}/streams"),
        Some(&viewer),
        None,
        Some(json!({"title": "Unauthorized Stream"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 2. Metadata updates are owner-only; ended streams are immutable.
    let (status, _) = request(
        &app,
        Method::PATCH,
        &format!("/v1/streams/{stream_id}"),
        Some(&viewer),
        None,
        Some(json!({"title": "Hijacked"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, updated) = request(
        &app,
        Method::PATCH,
        &format!("/v1/streams/{stream_id}"),
        Some(&owner),
        None,
        Some(json!({"title": "Updated Broadcast", "visibility": "unlisted"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["title"], "Updated Broadcast");
    assert_eq!(updated["visibility"], "unlisted");

    // 3. The internal media gateway can validate a publishing key without receiving stored plaintext.
    let (status, authorization) = request(
        &app,
        Method::POST,
        &format!("/internal/v1/streams/{stream_id}/authorize-publish"),
        None,
        None,
        Some(json!({"publishing_key": first_key})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{authorization}");
    assert!(authorization["authorized"].as_bool().unwrap());

    // 4. Publishing-key rotation replaces the prior credential and only returns plaintext once.
    let (status, rotated) = request(
        &app,
        Method::POST,
        &format!("/v1/streams/{stream_id}/publishing-key/rotate"),
        Some(&owner),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let second_key = rotated["publishing_key"].as_str().unwrap().to_owned();
    assert_ne!(first_key, second_key);
    let (status, _) = request(
        &app,
        Method::POST,
        &format!("/internal/v1/streams/{stream_id}/authorize-publish"),
        None,
        None,
        Some(json!({"publishing_key": first_key})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = request(
        &app,
        Method::POST,
        &format!("/internal/v1/streams/{stream_id}/authorize-publish"),
        None,
        None,
        Some(json!({"publishing_key": second_key})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let stored_hash: String = sqlx::query_scalar("SELECT key_hash FROM stream_publish_credentials WHERE stream_id = $1 AND revoked_at IS NULL")
        .bind(Uuid::parse_str(&stream_id).unwrap()).fetch_one(&pool).await.unwrap();
    assert_ne!(stored_hash, second_key);
    assert_eq!(stored_hash.len(), 64);

    // 4. Only trusted media events can transition the stream to live.
    let event_id = Uuid::new_v4();
    let event =
        json!({"event_id": event_id, "stream_id": stream_id, "event_type": "ingest_started"});
    let (status, _) = request(
        &app,
        Method::POST,
        "/internal/v1/media/events",
        None,
        None,
        Some(event.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = request(
        &app,
        Method::POST,
        "/internal/v1/media/events",
        None,
        Some(media_secret),
        Some(event.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Exact duplicate delivery is idempotent.
    let (status, _) = request(
        &app,
        Method::POST,
        "/internal/v1/media/events",
        None,
        Some(media_secret),
        Some(event.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // Reusing an event ID for a different payload is rejected.
    let (status, _) = request(
        &app,
        Method::POST,
        "/internal/v1/media/events",
        None,
        Some(media_secret),
        Some(json!({"event_id": event_id, "stream_id": stream_id, "event_type": "ingest_ended"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 5. Discovery contains only live, public, non-mature streams.
    let (status, _) = request(
        &app,
        Method::PATCH,
        &format!("/v1/streams/{stream_id}"),
        Some(&owner),
        None,
        Some(json!({"visibility": "public"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, discovered) = request(
        &app,
        Method::GET,
        "/v1/streams?category=gaming",
        None,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(discovered
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"] == stream_id));

    // 6. End event is also authenticated and idempotent.
    let end_id = Uuid::new_v4();
    let end_event =
        json!({"event_id": end_id, "stream_id": stream_id, "event_type": "ingest_ended"});
    let (status, _) = request(
        &app,
        Method::POST,
        "/internal/v1/media/events",
        None,
        Some(media_secret),
        Some(end_event.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = request(
        &app,
        Method::POST,
        "/internal/v1/media/events",
        None,
        Some(media_secret),
        Some(end_event),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, ended) = request(
        &app,
        Method::GET,
        &format!("/v1/streams/{stream_id}"),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ended["status"], "ended");

    // 7. Revocation removes the active publishing credential.
    let (status, _) = request(
        &app,
        Method::DELETE,
        &format!("/v1/streams/{stream_id}/publishing-key"),
        Some(&owner),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let active: i64 = sqlx::query_scalar("SELECT count(*) FROM stream_publish_credentials WHERE stream_id = $1 AND revoked_at IS NULL")
        .bind(Uuid::parse_str(&stream_id).unwrap()).fetch_one(&pool).await.unwrap();
    assert_eq!(active, 0);
    let (status, _) = request(
        &app,
        Method::POST,
        &format!("/internal/v1/streams/{stream_id}/authorize-publish"),
        None,
        None,
        Some(json!({"publishing_key": second_key})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Cleanup only test-owned rows, respecting foreign-key order.
    let stream_uuid = Uuid::parse_str(&stream_id).unwrap();
    let private_uuid = Uuid::parse_str(&private_id).unwrap();
    sqlx::query("DELETE FROM media_events WHERE stream_id IN ($1, $2)")
        .bind(stream_uuid)
        .bind(private_uuid)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM stream_publish_credentials WHERE stream_id IN ($1, $2)")
        .bind(stream_uuid)
        .bind(private_uuid)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM streams WHERE id IN ($1, $2)")
        .bind(stream_uuid)
        .bind(private_uuid)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM channels WHERE slug = $1")
        .bind(&slug)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email IN ($1, $2)")
        .bind(format!("stream-owner-{suffix}@example.test"))
        .bind(format!("stream-viewer-{suffix}@example.test"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn media_callbacks_fail_closed_when_internal_secret_is_missing() {
    let app = build_app(AppState::default(), &[]);
    let payload = json!({
        "event_id": Uuid::new_v4(),
        "stream_id": Uuid::new_v4(),
        "event_type": "ingest_started"
    });
    let (status, body) = request(
        &app,
        Method::POST,
        "/internal/v1/media/events",
        None,
        None,
        Some(payload),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
}
