use super::auth::{authenticated_user, ApiError};
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

const CATEGORIES: &[&str] = &[
    "gaming",
    "music",
    "talk",
    "education",
    "sports",
    "irl",
    "commerce",
];
const VISIBILITIES: &[&str] = &["public", "unlisted", "private"];

fn pool(state: &AppState) -> Result<&PgPool, ApiError> {
    state.db.as_ref().ok_or(ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
    ))
}

#[derive(Debug, FromRow, Serialize)]
pub struct StreamView {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub channel_slug: String,
    pub title: String,
    pub category: String,
    pub status: String,
    pub visibility: String,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(FromRow)]
struct OwnedStream {
    owner_user_id: Uuid,
    id: Uuid,
    channel_id: Uuid,
    channel_slug: String,
    title: String,
    category: String,
    status: String,
    visibility: String,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    ended_at: Option<chrono::DateTime<chrono::Utc>>,
    created_at: chrono::DateTime<chrono::Utc>,
}
impl From<OwnedStream> for StreamView {
    fn from(row: OwnedStream) -> Self {
        Self {
            id: row.id,
            channel_id: row.channel_id,
            channel_slug: row.channel_slug,
            title: row.title,
            category: row.category,
            status: row.status,
            visibility: row.visibility,
            started_at: row.started_at,
            ended_at: row.ended_at,
            created_at: row.created_at,
        }
    }
}

#[derive(Deserialize)]
pub struct CreateStream {
    title: String,
    category: Option<String>,
    visibility: Option<String>,
}
#[derive(Deserialize)]
pub struct UpdateStream {
    title: Option<String>,
    category: Option<String>,
    visibility: Option<String>,
}
#[derive(Deserialize)]
pub struct StreamQuery {
    q: Option<String>,
    category: Option<String>,
    limit: Option<i64>,
}
#[derive(Serialize)]
pub struct StreamCredentialResponse {
    stream: StreamView,
    publishing_key: String,
    publishing_key_notice: &'static str,
}
fn new_publishing_key() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    format!("lt_pub_{}", hex::encode(bytes))
}
fn credential_hash(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}
fn valid_title(title: &str) -> bool {
    (2..=120).contains(&title.trim().chars().count())
}

const STREAM_SELECT: &str = "SELECT s.id, s.channel_id, c.slug AS channel_slug, s.title, s.category, s.status, s.visibility, s.started_at, s.ended_at, s.created_at FROM streams s JOIN channels c ON c.id = s.channel_id";

pub async fn list_for_channel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<Vec<StreamView>>, ApiError> {
    let owner = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    let rows = sqlx::query_as::<_, StreamView>(
        "SELECT s.id, s.channel_id, c.slug AS channel_slug, s.title, s.category, s.status, s.visibility, s.started_at, s.ended_at, s.created_at FROM streams s JOIN channels c ON c.id = s.channel_id WHERE c.slug = $1 AND c.owner_user_id = $2 ORDER BY s.created_at DESC LIMIT 50"
    )
    .bind(slug)
    .bind(owner)
    .fetch_all(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stream_discovery_failed"))?;
    Ok(Json(rows))
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
    Json(input): Json<CreateStream>,
) -> Result<(StatusCode, Json<StreamCredentialResponse>), ApiError> {
    let owner = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    let title = input.title.trim();
    let category = input.category.unwrap_or_else(|| "talk".to_owned());
    let visibility = input.visibility.unwrap_or_else(|| "public".to_owned());
    if !valid_title(title) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_stream_title"));
    }
    if !CATEGORIES.contains(&category.as_str()) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_stream_category"));
    }
    if !VISIBILITIES.contains(&visibility.as_str()) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_stream_visibility",
        ));
    }

    let mut tx = db
        .begin()
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stream_create_failed"))?;
    let channel_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM channels WHERE slug = $1 AND owner_user_id = $2 FOR UPDATE",
    )
    .bind(&slug)
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stream_create_failed"))?
    .ok_or(ApiError(StatusCode::NOT_FOUND, "channel_not_found"))?;
    let row = sqlx::query_as::<_, StreamView>(
        "INSERT INTO streams (channel_id, title, category, visibility, status) VALUES ($1, $2, $3, $4, 'created') RETURNING id, channel_id, (SELECT slug FROM channels WHERE id = $1) AS channel_slug, title, category, status, visibility, started_at, ended_at, created_at"
    ).bind(channel_id).bind(title).bind(&category).bind(&visibility).fetch_one(&mut *tx).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stream_create_failed"))?;
    let key = new_publishing_key();
    sqlx::query("INSERT INTO stream_publish_credentials (stream_id, key_hash) VALUES ($1, $2)")
        .bind(row.id)
        .bind(credential_hash(&key))
        .execute(&mut *tx)
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "publishing_key_create_failed",
            )
        })?;
    tx.commit()
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stream_create_failed"))?;
    Ok((
        StatusCode::CREATED,
        Json(StreamCredentialResponse {
            stream: row,
            publishing_key: key,
            publishing_key_notice:
                "Store this key securely. It is shown only once and cannot be recovered.",
        }),
    ))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(stream_id): Path<Uuid>,
    Json(input): Json<UpdateStream>,
) -> Result<Json<StreamView>, ApiError> {
    let owner = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    let title = input.title.map(|value| value.trim().to_owned());
    if title.as_ref().is_some_and(|value| !valid_title(value)) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_stream_title"));
    }
    if input
        .category
        .as_ref()
        .is_some_and(|value| !CATEGORIES.contains(&value.as_str()))
    {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_stream_category"));
    }
    if input
        .visibility
        .as_ref()
        .is_some_and(|value| !VISIBILITIES.contains(&value.as_str()))
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_stream_visibility",
        ));
    }
    let row = sqlx::query_as::<_, StreamView>(
        "UPDATE streams s SET title = COALESCE($3, s.title), category = COALESCE($4, s.category), visibility = COALESCE($5, s.visibility), updated_at = now() FROM channels c WHERE s.id = $1 AND c.id = s.channel_id AND c.owner_user_id = $2 AND s.status <> 'ended' RETURNING s.id, s.channel_id, c.slug AS channel_slug, s.title, s.category, s.status, s.visibility, s.started_at, s.ended_at, s.created_at"
    ).bind(stream_id).bind(owner).bind(title).bind(input.category).bind(input.visibility)
        .fetch_optional(db).await.map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stream_update_failed"))?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "stream_not_found"))?;
    Ok(Json(row))
}

pub async fn discover(
    State(state): State<AppState>,
    Query(query): Query<StreamQuery>,
) -> Result<Json<Vec<StreamView>>, ApiError> {
    let db = pool(&state)?;
    if query
        .category
        .as_ref()
        .is_some_and(|value| !CATEGORIES.contains(&value.as_str()))
    {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_stream_category"));
    }
    let search = query
        .q
        .unwrap_or_default()
        .trim()
        .chars()
        .take(80)
        .collect::<String>();
    let pattern = format!("%{}%", search.replace('%', "\\%").replace('_', "\\_"));
    let limit = query.limit.unwrap_or(24).clamp(1, 50);
    let sql = format!("{STREAM_SELECT} JOIN channels public_channel ON public_channel.id = s.channel_id WHERE s.status = 'live' AND s.visibility = 'public' AND public_channel.is_mature = false AND ($1 = '%%' OR s.title ILIKE $1 OR c.slug ILIKE $1) AND ($2::text IS NULL OR s.category = $2) ORDER BY s.started_at DESC NULLS LAST LIMIT $3");
    let rows = sqlx::query_as::<_, StreamView>(&sql)
        .bind(pattern)
        .bind(query.category)
        .bind(limit)
        .fetch_all(db)
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stream_discovery_failed"))?;
    Ok(Json(rows))
}

pub async fn get_public(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(stream_id): Path<Uuid>,
) -> Result<Json<StreamView>, ApiError> {
    let db = pool(&state)?;
    let row = sqlx::query_as::<_, OwnedStream>(
        "SELECT c.owner_user_id, s.id, s.channel_id, c.slug AS channel_slug, s.title, s.category, s.status, s.visibility, s.started_at, s.ended_at, s.created_at FROM streams s JOIN channels c ON c.id = s.channel_id WHERE s.id = $1"
    ).bind(stream_id).fetch_optional(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stream_read_failed"))?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "stream_not_found"))?;
    if row.visibility == "private"
        && authenticated_user(&headers, &state).ok() != Some(row.owner_user_id)
    {
        return Err(ApiError(StatusCode::NOT_FOUND, "stream_not_found"));
    }
    Ok(Json(row.into()))
}

async fn verify_owner(db: &PgPool, stream_id: Uuid, owner: Uuid) -> Result<(), ApiError> {
    let owns = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM streams s JOIN channels c ON c.id = s.channel_id WHERE s.id = $1 AND c.owner_user_id = $2)")
        .bind(stream_id).bind(owner).fetch_one(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "stream_authorization_failed"))?;
    if !owns {
        return Err(ApiError(StatusCode::NOT_FOUND, "stream_not_found"));
    }
    Ok(())
}

pub async fn rotate_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(stream_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let owner = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    verify_owner(db, stream_id, owner).await?;
    let status = sqlx::query_scalar::<_, String>("SELECT status FROM streams WHERE id = $1")
        .bind(stream_id)
        .fetch_one(db)
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "publishing_key_rotation_failed",
            )
        })?;
    if status == "ended" {
        return Err(ApiError(StatusCode::CONFLICT, "stream_already_ended"));
    }
    let mut tx = db.begin().await.map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "publishing_key_rotation_failed",
        )
    })?;
    sqlx::query("UPDATE stream_publish_credentials SET revoked_at = now() WHERE stream_id = $1 AND revoked_at IS NULL")
        .bind(stream_id).execute(&mut *tx).await.map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "publishing_key_rotation_failed"))?;
    let key = new_publishing_key();
    sqlx::query("INSERT INTO stream_publish_credentials (stream_id, key_hash) VALUES ($1, $2)")
        .bind(stream_id)
        .bind(credential_hash(&key))
        .execute(&mut *tx)
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "publishing_key_rotation_failed",
            )
        })?;
    tx.commit().await.map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "publishing_key_rotation_failed",
        )
    })?;
    Ok(Json(
        serde_json::json!({"publishing_key": key, "notice": "Store this key securely. It is shown only once."}),
    ))
}

pub async fn revoke_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(stream_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let owner = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    verify_owner(db, stream_id, owner).await?;
    sqlx::query("UPDATE stream_publish_credentials SET revoked_at = now() WHERE stream_id = $1 AND revoked_at IS NULL")
        .bind(stream_id).execute(db).await.map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "publishing_key_revoke_failed"))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PublishAuthorization {
    publishing_key: String,
}
pub async fn authorize_publish(
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
    Json(input): Json<PublishAuthorization>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if input.publishing_key.len() != 71
        || !input.publishing_key.starts_with("lt_pub_")
        || !input.publishing_key[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_publishing_key"));
    }
    let db = pool(&state)?;
    let authorized = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT s.id, c.slug, s.status FROM streams s JOIN channels c ON c.id = s.channel_id JOIN stream_publish_credentials p ON p.stream_id = s.id WHERE s.id = $1 AND p.key_hash = $2 AND p.revoked_at IS NULL AND s.status IN ('created', 'live')"
    ).bind(stream_id).bind(credential_hash(&input.publishing_key)).fetch_optional(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "publish_authorization_failed"))?
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_publishing_key"))?;
    Ok(Json(
        serde_json::json!({"authorized": true, "stream_id": authorized.0, "channel_slug": authorized.1, "status": authorized.2}),
    ))
}

#[derive(Deserialize)]
pub struct MediaEvent {
    event_id: Uuid,
    stream_id: Uuid,
    event_type: String,
}
fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (*a ^ *b))
        == 0
}

pub async fn media_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(event): Json<MediaEvent>,
) -> Result<StatusCode, ApiError> {
    let Some(secret) = state.media_event_secret.as_deref() else {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "media_events_not_configured",
        ));
    };
    let supplied = headers
        .get("x-live-tron-media-secret")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !constant_time_equal(secret.as_bytes(), supplied.as_bytes()) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "invalid_media_event_secret",
        ));
    }
    if event.event_type != "ingest_started" && event.event_type != "ingest_ended" {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_media_event_type",
        ));
    }
    let db = pool(&state)?;
    let mut tx = db
        .begin()
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "media_event_failed"))?;
    let inserted = sqlx::query("INSERT INTO media_events (event_id, stream_id, event_type) VALUES ($1, $2, $3) ON CONFLICT (event_id) DO NOTHING")
        .bind(event.event_id).bind(event.stream_id).bind(&event.event_type).execute(&mut *tx).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "media_event_failed"))?;
    if inserted.rows_affected() == 0 {
        let existing = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT stream_id, event_type FROM media_events WHERE event_id = $1",
        )
        .bind(event.event_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "media_event_failed"))?;
        if existing.0 == event.stream_id && existing.1 == event.event_type {
            tx.commit()
                .await
                .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "media_event_failed"))?;
            return Ok(StatusCode::NO_CONTENT);
        }
        return Err(ApiError(StatusCode::CONFLICT, "media_event_id_reused"));
    }
    let updated = match event.event_type.as_str() {
        "ingest_started" => sqlx::query("UPDATE streams SET status = 'live', started_at = COALESCE(started_at, now()), updated_at = now() WHERE id = $1 AND status = 'created'")
            .bind(event.stream_id).execute(&mut *tx).await,
        "ingest_ended" => sqlx::query("UPDATE streams SET status = 'ended', ended_at = now(), updated_at = now() WHERE id = $1 AND status = 'live'")
            .bind(event.stream_id).execute(&mut *tx).await,
        _ => unreachable!(),
    }.map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "media_event_failed"))?;
    if updated.rows_affected() != 1 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "invalid_stream_lifecycle_transition",
        ));
    }
    tx.commit()
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "media_event_failed"))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::{constant_time_equal, credential_hash, new_publishing_key};
    #[test]
    fn publishing_credentials_are_random_prefixed_and_only_hashable() {
        let first = new_publishing_key();
        let second = new_publishing_key();
        assert!(first.starts_with("lt_pub_"));
        assert_eq!(first.len(), 71);
        assert_ne!(first, second);
        assert_ne!(credential_hash(&first), first);
    }
    #[test]
    fn media_secret_comparison_requires_equal_bytes() {
        assert!(constant_time_equal(b"same-secret", b"same-secret"));
        assert!(!constant_time_equal(b"same-secret", b"wrong-secret"));
        assert!(!constant_time_equal(b"short", b"longer"));
    }
}
