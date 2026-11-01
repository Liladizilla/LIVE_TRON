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

async fn register(app: &Router, email: String) -> String {
    let (status, tokens) = request(
        app,
        Method::POST,
        "/v1/auth/register",
        None,
        None,
        Some(json!({
            "email": email,
            "password": "a-secure-test-password-123",
            "display_name": "Chat Tester"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{tokens}");
    tokens["access_token"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn persisted_chat_enforces_access_validation_and_rate_limits() {
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
    let media_secret = "integration-chat-media-secret-with-32-bytes";
    let app = build_app(
        AppState::new(Some(pool))
            .with_jwt_secret(Some(
                "integration-chat-jwt-secret-with-at-least-32-bytes".to_owned(),
            ))
            .with_media_event_secret(Some(media_secret.to_owned())),
        &[],
    );
    let owner = register(&app, format!("chat-{suffix}@example.test")).await;
    let slug = format!("chat-{suffix}");
    let (status, channel) = request(
        &app,
        Method::POST,
        "/v1/channels",
        Some(&owner),
        None,
        Some(json!({"slug": slug, "title": "Chat Test Channel", "category": "talk"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{channel}");

    let (status, created) = request(
        &app,
        Method::POST,
        &format!("/v1/channels/{slug}/streams"),
        Some(&owner),
        None,
        Some(json!({"title": "Chat Test Broadcast", "category": "talk", "visibility": "public"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let stream_id = created["stream"]["id"].as_str().unwrap();
    let event = json!({
        "event_id": Uuid::new_v4(),
        "stream_id": stream_id,
        "event_type": "ingest_started"
    });
    let (status, _) = request(
        &app,
        Method::POST,
        "/internal/v1/media/events",
        None,
        Some(media_secret),
        Some(event),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let chat_uri = format!("/v1/streams/{stream_id}/chat");
    let (status, _) = request(
        &app,
        Method::POST,
        &chat_uri,
        None,
        None,
        Some(json!({"body": "anonymous spam"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    for index in 0..5 {
        let (status, message) = request(
            &app,
            Method::POST,
            &chat_uri,
            Some(&owner),
            None,
            Some(json!({"body": format!("message {index}")})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{message}");
        assert_eq!(message["body"], format!("message {index}"));
    }

    let (status, limited) = request(
        &app,
        Method::POST,
        &chat_uri,
        Some(&owner),
        None,
        Some(json!({"body": "sixth message"})),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{limited}");
    assert_eq!(limited["error"], "chat_rate_limited");

    let (status, history) = request(&app, Method::GET, &chat_uri, None, None, None).await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(history.as_array().unwrap().len(), 5);
    assert_eq!(history[0]["body"], "message 0");
    assert_eq!(history[4]["body"], "message 4");

    let (status, _) = request(
        &app,
        Method::POST,
        &chat_uri,
        Some(&owner),
        None,
        Some(json!({"body": "x".repeat(501)})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
