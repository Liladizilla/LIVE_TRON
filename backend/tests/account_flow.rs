use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
};
use live_tron_api::{build_app, state::AppState};
use serde_json::{json, Value};
use sqlx::{migrate::Migrator, postgres::PgPoolOptions};
use std::path::Path;
use tower::ServiceExt;
use uuid::Uuid;

async fn json_response(response: axum::response::Response) -> (StatusCode, Value) {
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    (status, value)
}

fn request(method: Method, uri: &str, body: Value, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn account_session_creator_and_follow_flow() {
    let Ok(database_url) = std::env::var("DATABASE_URL") else {
        eprintln!("Skipping PostgreSQL integration test: DATABASE_URL is not set");
        return;
    };

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("test database must be reachable");
    let migration_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let migrator = Migrator::new(migration_path)
        .await
        .expect("migration directory must load");
    migrator.run(&pool).await.expect("schema must be current");

    let secret = "integration-test-secret-with-at-least-32-bytes";
    let app = build_app(
        AppState::new(Some(pool.clone())).with_jwt_secret(Some(secret.to_owned())),
        &[],
    );

    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("creator-{suffix}@example.test");
    let password = "a-long-integration-password";
    let register = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/auth/register",
            json!({
                "email": email,
                "password": password,
                "display_name": "Test Creator"
            }),
            None,
        ))
        .await
        .unwrap();
    let (status, registered) = json_response(register).await;
    assert_eq!(status, StatusCode::CREATED);
    let access = registered["access_token"].as_str().unwrap().to_owned();
    let refresh = registered["refresh_token"].as_str().unwrap().to_owned();
    let user_id = registered["user"]["id"].as_str().unwrap();

    let profile = app
        .clone()
        .oneshot(request(Method::GET, "/v1/me", json!({}), Some(&access)))
        .await
        .unwrap();
    let (status, profile) = json_response(profile).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(profile["id"], user_id);

    let updated_profile = app
        .clone()
        .oneshot(request(
            Method::PATCH,
            "/v1/me",
            json!({ "display_name": "Updated Creator" }),
            Some(&access),
        ))
        .await
        .unwrap();
    let (status, updated_profile) = json_response(updated_profile).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_profile["display_name"], "Updated Creator");

    let login = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/auth/login",
            json!({ "email": email, "password": password }),
            None,
        ))
        .await
        .unwrap();
    let (status, login) = json_response(login).await;
    assert_eq!(status, StatusCode::OK);
    assert!(login["access_token"].as_str().is_some());

    let rotated = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/auth/refresh",
            json!({ "refresh_token": refresh }),
            None,
        ))
        .await
        .unwrap();
    let (status, rotated) = json_response(rotated).await;
    assert_eq!(status, StatusCode::OK);
    let rotated_access = rotated["access_token"].as_str().unwrap().to_owned();
    let rotated_refresh = rotated["refresh_token"].as_str().unwrap().to_owned();

    let replay = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/auth/refresh",
            json!({ "refresh_token": refresh }),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);

    let slug = format!("creator-{suffix}");
    let create_channel = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/channels",
            json!({
                "slug": slug,
                "title": "Test Creator Channel",
                "description": "An integration test channel",
                "category": "gaming"
            }),
            Some(&rotated_access),
        ))
        .await
        .unwrap();
    let (status, channel) = json_response(create_channel).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(channel["slug"], slug);

    let owner_update = app
        .clone()
        .oneshot(request(
            Method::PATCH,
            &format!("/v1/channels/{slug}"),
            json!({ "title": "Updated Creator Channel" }),
            Some(&rotated_access),
        ))
        .await
        .unwrap();
    let (status, owner_update) = json_response(owner_update).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_update["title"], "Updated Creator Channel");

    let creator_profile = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/v1/creators/{slug}"),
            json!({}),
            None,
        ))
        .await
        .unwrap();
    let (status, creator_profile) = json_response(creator_profile).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(creator_profile["display_name"], "Updated Creator");

    let second_email = format!("viewer-{suffix}@example.test");
    let second_user = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/auth/register",
            json!({
                "email": second_email,
                "password": password,
                "display_name": "Test Viewer"
            }),
            None,
        ))
        .await
        .unwrap();
    let (status, second_user) = json_response(second_user).await;
    assert_eq!(status, StatusCode::CREATED);
    let viewer_token = second_user["access_token"].as_str().unwrap();

    let forbidden_update = app
        .clone()
        .oneshot(request(
            Method::PATCH,
            &format!("/v1/channels/{slug}"),
            json!({ "title": "Hijacked Channel" }),
            Some(viewer_token),
        ))
        .await
        .unwrap();
    assert_eq!(forbidden_update.status(), StatusCode::NOT_FOUND);

    let follow = app
        .clone()
        .oneshot(request(
            Method::POST,
            &format!("/v1/channels/{slug}/follow"),
            json!({}),
            Some(viewer_token),
        ))
        .await
        .unwrap();
    assert_eq!(follow.status(), StatusCode::NO_CONTENT);

    let discover = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/v1/channels?q={slug}&category=gaming"),
            json!({}),
            None,
        ))
        .await
        .unwrap();
    let (status, channels) = json_response(discover).await;
    assert_eq!(status, StatusCode::OK);
    assert!(channels
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["slug"] == slug));

    let unfollow = app
        .clone()
        .oneshot(request(
            Method::DELETE,
            &format!("/v1/channels/{slug}/follow"),
            json!({}),
            Some(viewer_token),
        ))
        .await
        .unwrap();
    assert_eq!(unfollow.status(), StatusCode::NO_CONTENT);

    let logout = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/auth/logout",
            json!({ "refresh_token": rotated_refresh }),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);

    let revoked_replay = app
        .clone()
        .oneshot(request(
            Method::POST,
            "/v1/auth/refresh",
            json!({ "refresh_token": rotated_refresh }),
            None,
        ))
        .await
        .unwrap();
    assert_eq!(revoked_replay.status(), StatusCode::UNAUTHORIZED);

    sqlx::query("DELETE FROM channels WHERE slug = $1")
        .bind(&slug)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM users WHERE email IN ($1, $2)")
        .bind(&email)
        .bind(&second_email)
        .execute(&pool)
        .await
        .unwrap();

    pool.close().await;
}
