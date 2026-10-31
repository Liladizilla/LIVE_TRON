use super::auth::{authenticated_user, ApiError};
use crate::state::AppState;
use axum::{extract::{Path, Query, State}, http::{HeaderMap, StatusCode}, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

const CATEGORIES: &[&str] = &["gaming", "music", "talk", "education", "sports", "irl", "commerce"];
fn pool(state: &AppState) -> Result<&PgPool, ApiError> {
    state.db.as_ref().ok_or(ApiError(StatusCode::SERVICE_UNAVAILABLE, "database_unavailable"))
}
#[derive(Deserialize)]
pub struct CreateChannel {
    slug: String, title: String, description: Option<String>,
    category: Option<String>, is_mature: Option<bool>,
}
#[derive(Deserialize)]
pub struct UpdateChannel {
    title: Option<String>, description: Option<String>,
    category: Option<String>, is_mature: Option<bool>,
}
#[derive(FromRow, Serialize)]
pub struct ChannelView {
    pub id: Uuid, pub owner_user_id: Uuid, pub slug: String, pub title: String,
    pub description: String, pub category: String, pub is_mature: bool,
    pub follower_count: i64,
}
fn validate_channel(title: &str, description: &str, category: &str) -> Result<(), ApiError> {
    if !(2..=100).contains(&title.trim().chars().count()) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_channel_title"));
    }
    if description.chars().count() > 2000 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "channel_description_too_long"));
    }
    if !CATEGORIES.contains(&category) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_channel_category"));
    }
    Ok(())
}
pub async fn create(State(state): State<AppState>, headers: HeaderMap, Json(input): Json<CreateChannel>)
    -> Result<(StatusCode, Json<ChannelView>), ApiError> {
    let owner = authenticated_user(&headers, &state)?; let db = pool(&state)?;
    let slug = input.slug.trim().to_lowercase(); let title = input.title.trim();
    let description = input.description.unwrap_or_default();
    let category = input.category.unwrap_or_else(|| "talk".to_owned());
    validate_channel(title, &description, &category)?;
    if !(3..=48).contains(&slug.len())
        || !slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        || !slug.as_bytes()[0].is_ascii_alphanumeric() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_channel_slug"));
    }
    let channel = sqlx::query_as::<_, ChannelView>("INSERT INTO channels (owner_user_id, slug, title, description, category, is_mature) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id, owner_user_id, slug, title, description, category, is_mature, 0::bigint AS follower_count")
        .bind(owner).bind(slug).bind(title).bind(description).bind(category)
        .bind(input.is_mature.unwrap_or(false)).fetch_one(db).await.map_err(|error| {
            if error.as_database_error().is_some_and(|e| e.code().as_deref() == Some("23505")) {
                ApiError(StatusCode::CONFLICT, "channel_slug_taken")
            } else { tracing::error!(error = %error, "channel creation failed"); ApiError(StatusCode::INTERNAL_SERVER_ERROR, "channel_create_failed") }
        })?;
    Ok((StatusCode::CREATED, Json(channel)))
}
pub async fn update(State(state): State<AppState>, headers: HeaderMap, Path(slug): Path<String>, Json(input): Json<UpdateChannel>)
    -> Result<Json<ChannelView>, ApiError> {
    let owner = authenticated_user(&headers, &state)?; let db = pool(&state)?;
    let title = input.title.map(|v| v.trim().to_owned());
    if title.as_ref().is_some_and(|v| !(2..=100).contains(&v.chars().count())) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_channel_title"));
    }
    if input.description.as_ref().is_some_and(|v| v.chars().count() > 2000) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "channel_description_too_long"));
    }
    if input.category.as_ref().is_some_and(|v| !CATEGORIES.contains(&v.as_str())) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_channel_category"));
    }
    let channel = sqlx::query_as::<_, ChannelView>("UPDATE channels c SET title = COALESCE($3, c.title), description = COALESCE($4, c.description), category = COALESCE($5, c.category), is_mature = COALESCE($6, c.is_mature), updated_at = now() WHERE c.slug = $1 AND c.owner_user_id = $2 RETURNING c.id, c.owner_user_id, c.slug, c.title, c.description, c.category, c.is_mature, (SELECT count(*) FROM follows f WHERE f.channel_id = c.id)::bigint AS follower_count")
        .bind(slug).bind(owner).bind(title).bind(input.description).bind(input.category).bind(input.is_mature)
        .fetch_optional(db).await.map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "channel_update_failed"))?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "channel_not_found"))?;
    Ok(Json(channel))
}
#[derive(Deserialize)]
pub struct DiscoveryQuery { q: Option<String>, category: Option<String>, limit: Option<i64> }
pub async fn discover(State(state): State<AppState>, Query(query): Query<DiscoveryQuery>)
    -> Result<Json<Vec<ChannelView>>, ApiError> {
    let db = pool(&state)?;
    if query.category.as_ref().is_some_and(|c| !CATEGORIES.contains(&c.as_str())) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_channel_category"));
    }
    let search = query.q.unwrap_or_default().trim().chars().take(80).collect::<String>();
    let pattern = format!("%{}%", search.replace('%', "\\%").replace('_', "\\_"));
    let limit = query.limit.unwrap_or(24).clamp(1, 50);
    let rows = sqlx::query_as::<_, ChannelView>("SELECT c.id, c.owner_user_id, c.slug, c.title, c.description, c.category, c.is_mature, count(f.follower_user_id)::bigint AS follower_count FROM channels c LEFT JOIN follows f ON f.channel_id = c.id WHERE ($1 = '%%' OR c.title ILIKE $1 OR c.slug ILIKE $1) AND ($2::text IS NULL OR c.category = $2) GROUP BY c.id ORDER BY follower_count DESC, c.created_at DESC LIMIT $3")
        .bind(pattern).bind(query.category).bind(limit).fetch_all(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "channel_discovery_failed"))?;
    Ok(Json(rows))
}
pub async fn get_public(State(state): State<AppState>, Path(slug): Path<String>)
    -> Result<Json<ChannelView>, ApiError> {
    let db = pool(&state)?;
    let channel = sqlx::query_as::<_, ChannelView>("SELECT c.id, c.owner_user_id, c.slug, c.title, c.description, c.category, c.is_mature, count(f.follower_user_id)::bigint AS follower_count FROM channels c LEFT JOIN follows f ON f.channel_id = c.id WHERE c.slug = $1 GROUP BY c.id")
        .bind(slug).fetch_optional(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "channel_read_failed"))?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "channel_not_found"))?;
    Ok(Json(channel))
}
pub async fn follow(State(state): State<AppState>, headers: HeaderMap, Path(slug): Path<String>)
    -> Result<StatusCode, ApiError> {
    let follower = authenticated_user(&headers, &state)?; let db = pool(&state)?;
    let result = sqlx::query("INSERT INTO follows (follower_user_id, channel_id) SELECT $1, id FROM channels WHERE slug = $2 AND owner_user_id <> $1 ON CONFLICT DO NOTHING")
        .bind(follower).bind(&slug).execute(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "follow_failed"))?;
    if result.rows_affected() == 0 {
        let channel = sqlx::query_as::<_, (Uuid, Uuid)>("SELECT id, owner_user_id FROM channels WHERE slug = $1")
            .bind(&slug).fetch_optional(db).await
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "follow_failed"))?
            .ok_or(ApiError(StatusCode::NOT_FOUND, "channel_not_found"))?;
        if channel.1 == follower { return Err(ApiError(StatusCode::BAD_REQUEST, "cannot_follow_own_channel")); }
    }
    Ok(StatusCode::NO_CONTENT)
}
pub async fn unfollow(State(state): State<AppState>, headers: HeaderMap, Path(slug): Path<String>)
    -> Result<StatusCode, ApiError> {
    let follower = authenticated_user(&headers, &state)?; let db = pool(&state)?;
    sqlx::query("DELETE FROM follows f USING channels c WHERE f.channel_id = c.id AND c.slug = $1 AND f.follower_user_id = $2")
        .bind(slug).bind(follower).execute(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "unfollow_failed"))?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn creator_profile(State(state): State<AppState>, Path(slug): Path<String>)
    -> Result<Json<serde_json::Value>, ApiError> {
    let db = pool(&state)?;
    let profile = sqlx::query_as::<_, (Uuid, String, Option<String>, chrono::DateTime<chrono::Utc>)>("SELECT u.id, u.display_name, u.avatar_url, u.created_at FROM users u JOIN channels c ON c.owner_user_id = u.id WHERE c.slug = $1")
        .bind(&slug).fetch_optional(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "creator_read_failed"))?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "creator_not_found"))?;
    let channels = sqlx::query_as::<_, ChannelView>("SELECT c.id, c.owner_user_id, c.slug, c.title, c.description, c.category, c.is_mature, count(f.follower_user_id)::bigint AS follower_count FROM channels c LEFT JOIN follows f ON f.channel_id = c.id WHERE c.owner_user_id = $1 GROUP BY c.id ORDER BY c.created_at DESC")
        .bind(profile.0).fetch_all(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "creator_channels_read_failed"))?;
    Ok(Json(json!({ "id": profile.0, "display_name": profile.1, "avatar_url": profile.2, "created_at": profile.3, "channels": channels })))
}
