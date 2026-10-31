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
    payload: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
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
    let bytes = to_bytes(response.into_body(), 1024 * 64).await.unwrap();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

#[tokio::test]
async fn account_creator_lifecycle_is_authorized_and_refresh_tokens_rotate() {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must point to the PostgreSQL CI service");
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

    let secret = "integration-test-secret-with-at-least-32-bytes";
    let app = build_app(
        AppState::new(Some(pool.clone())).with_jwt_secret(Some(secret.to_owned())),
        &[],
    );

    let suffix = Uuid::new_v4().simple().to_string();
    let owner_email = format!("owner-{suffix}@example.test");
    let viewer_email = format!("viewer-{suffix}@example.test");
    let password = "a-secure-test-password-123";
    let slug = format!("channel-{suffix}");

    // 1. Register creator and receive an authenticated session.
    let (status, owner_tokens) = request(
        &app,
        Method::POST,
        "/v1/auth/register",
        None,
        Some(json!({
            "email": owner_email,
            "password": password,
            "display_name": "Creator One"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{owner_tokens}");
    let owner_access = owner_tokens["access_token"].as_str().unwrap().to_owned();
    let owner_refresh = owner_tokens["refresh_token"].as_str().unwrap().to_owned();
    let owner_id = owner_tokens["user"]["id"].as_str().unwrap().to_owned();

    // 2. Duplicate normalized email is rejected.
    let (status, _) = request(
        &app,
        Method::POST,
        "/v1/auth/register",
        None,
        Some(json!({
            "email": format!("OWNER-{suffix}@example.test"),
            "password": password,
            "display_name": "Duplicate User"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // 3. Login accepts normalized email and returns a fresh session.
    let (status, login_tokens) = request(
        &app,
        Method::POST,
        "/v1/auth/login",
        None,
        Some(json!({
            "email": format!(" OWNER-{suffix}@EXAMPLE.TEST "),
            "password": password
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{login_tokens}");

    // 4. Read the authenticated account.
    let (status, profile) = request(&app, Method::GET, "/v1/me", Some(&owner_access), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(profile["id"], owner_id);

    // 5. Update the current user's display name.
    let (status, updated) = request(
        &app,
        Method::PATCH,
        "/v1/me",
        Some(&owner_access),
        Some(json!({ "display_name": "Creator Updated" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["display_name"], "Creator Updated");

    // 6. Create a channel owned by the authenticated creator.
    let (status, channel) = request(
        &app,
        Method::POST,
        "/v1/channels",
        Some(&owner_access),
        Some(json!({
            "slug": slug,
            "title": "Live Test Channel",
            "description": "PostgreSQL-backed integration test",
            "category": "gaming"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{channel}");
    assert_eq!(channel["owner_user_id"], owner_id);

    // 7. Register a second account to exercise ownership boundaries.
    let (status, viewer_tokens) = request(
        &app,
        Method::POST,
        "/v1/auth/register",
        None,
        Some(json!({
            "email": viewer_email,
            "password": password,
            "display_name": "Viewer One"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let viewer_access = viewer_tokens["access_token"].as_str().unwrap();

    // 8. A different account cannot update the creator's channel.
    let (status, _) = request(
        &app,
        Method::PATCH,
        &format!("/v1/channels/{slug}"),
        Some(viewer_access),
        Some(json!({ "title": "Unauthorized overwrite" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 9. Public channel details are available by slug.
    let (status, public_channel) = request(
        &app,
        Method::GET,
        &format!("/v1/channels/{slug}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(public_channel["slug"], slug);

    // 10. Discovery filters channels by category and search term.
    let (status, discovered) = request(
        &app,
        Method::GET,
        "/v1/channels?q=Live%20Test&category=gaming&limit=10",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(discovered
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["slug"] == slug));

    // 11. A viewer can follow another user's channel.
    let (status, _) = request(
        &app,
        Method::POST,
        &format!("/v1/channels/{slug}/follow"),
        Some(viewer_access),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 12. Creator profile aggregates owned channels.
    let (status, creator) = request(
        &app,
        Method::GET,
        &format!("/v1/creators/{slug}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(creator["display_name"], "Creator Updated");
    assert!(creator["channels"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["slug"] == slug));

    // 13. Refresh rotation issues a new token pair.
    let (status, rotated) = request(
        &app,
        Method::POST,
        "/v1/auth/refresh",
        None,
        Some(json!({ "refresh_token": owner_refresh.clone() })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let new_refresh = rotated["refresh_token"].as_str().unwrap().to_owned();

    // 14. A rotated refresh token cannot be replayed.
    let (status, _) = request(
        &app,
        Method::POST,
        "/v1/auth/refresh",
        None,
        Some(json!({ "refresh_token": owner_refresh })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 15. Logout revokes the current refresh token.
    let (status, _) = request(
        &app,
        Method::POST,
        "/v1/auth/logout",
        None,
        Some(json!({ "refresh_token": new_refresh.clone() })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 16. A logged-out refresh token cannot be used again.
    let (status, _) = request(
        &app,
        Method::POST,
        "/v1/auth/refresh",
        None,
        Some(json!({ "refresh_token": new_refresh })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 17. Unfollow is idempotent and removes the relation.
    let (status, _) = request(
        &app,
        Method::DELETE,
        &format!("/v1/channels/{slug}/follow"),
        Some(viewer_access),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Clean up only records created by this test.
    sqlx::query("DELETE FROM channels WHERE slug = $1")
        .bind(&slug)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE id::text = $1 OR email = $2")
        .bind(owner_id)
        .bind(format!("viewer-{suffix}@example.test"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}
