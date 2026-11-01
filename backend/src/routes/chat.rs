use super::auth::{authenticated_user, ApiError};
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

fn pool(state: &AppState) -> Result<&PgPool, ApiError> {
    state.db.as_ref().ok_or(ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
    ))
}

#[derive(Debug, FromRow, Serialize)]
pub struct ChatMessage {
    pub id: Uuid,
    pub stream_id: Uuid,
    pub user_id: Uuid,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    before: Option<DateTime<Utc>>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct SendMessage {
    body: String,
}

async fn ensure_chat_access(
    db: &PgPool,
    stream_id: Uuid,
    viewer: Option<Uuid>,
) -> Result<(), ApiError> {
    let access = sqlx::query_as::<_, (String, String, Uuid)>(
        "SELECT s.status, s.visibility, c.owner_user_id FROM streams s JOIN channels c ON c.id = s.channel_id WHERE s.id = $1"
    )
    .bind(stream_id)
    .fetch_optional(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_access_check_failed"))?
    .ok_or(ApiError(StatusCode::NOT_FOUND, "stream_not_found"))?;

    if access.0 != "live" {
        return Err(ApiError(StatusCode::CONFLICT, "chat_stream_not_live"));
    }
    if access.1 == "private" && viewer != Some(access.2) {
        return Err(ApiError(StatusCode::NOT_FOUND, "stream_not_found"));
    }
    Ok(())
}

pub async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(stream_id): Path<Uuid>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<ChatMessage>>, ApiError> {
    let db = pool(&state)?;
    let viewer = authenticated_user(&headers, &state).ok();
    ensure_chat_access(db, stream_id, viewer).await?;
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let rows = sqlx::query_as::<_, ChatMessage>(
        "SELECT m.id, m.stream_id, m.user_id, u.display_name, u.avatar_url, m.body, m.created_at FROM chat_messages m JOIN users u ON u.id = m.user_id WHERE m.stream_id = $1 AND ($2::timestamptz IS NULL OR m.created_at < $2) ORDER BY m.created_at DESC, m.id DESC LIMIT $3"
    )
    .bind(stream_id)
    .bind(query.before)
    .bind(limit)
    .fetch_all(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_history_failed"))?;
    Ok(Json(rows.into_iter().rev().collect()))
}

pub async fn send(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(stream_id): Path<Uuid>,
    Json(input): Json<SendMessage>,
) -> Result<(StatusCode, Json<ChatMessage>), ApiError> {
    let user_id = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    let body = input.body.trim();
    if body.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "chat_message_empty"));
    }
    if body.chars().count() > 500 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "chat_message_too_long"));
    }
    ensure_chat_access(db, stream_id, Some(user_id)).await?;

    let mut tx = db
        .begin()
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_send_failed"))?;
    let lock_key = format!("{stream_id}:{user_id}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(lock_key)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_send_failed"))?;
    let recent_count = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM chat_messages WHERE stream_id = $1 AND user_id = $2 AND created_at > now() - interval '10 seconds'"
    )
    .bind(stream_id)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_send_failed"))?;
    if recent_count >= 5 {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "chat_rate_limited"));
    }
    let message = sqlx::query_as::<_, ChatMessage>(
        "INSERT INTO chat_messages (stream_id, user_id, body) SELECT $1, u.id, $3 FROM users u WHERE u.id = $2 RETURNING id, stream_id, user_id, (SELECT display_name FROM users WHERE id = $2) AS display_name, (SELECT avatar_url FROM users WHERE id = $2) AS avatar_url, body, created_at"
    )
    .bind(stream_id)
    .bind(user_id)
    .bind(body)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_send_failed"))?;
    tx.commit()
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_send_failed"))?;
    Ok((StatusCode::CREATED, Json(message)))
}
