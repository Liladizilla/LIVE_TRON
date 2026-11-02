use super::auth::{authenticated_user, ApiError};
use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{
        header::{ORIGIN, SEC_WEBSOCKET_PROTOCOL},
        HeaderMap, StatusCode,
    },
    response::Response,
    Json,
};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgListener, FromRow, PgPool};
use std::time::Duration;
use tokio::sync::broadcast;
use uuid::Uuid;

const CHAT_TICKET_TTL: Duration = Duration::from_secs(60);
const CHAT_TICKET_PREFIX: &str = "live-tron.chat.ticket.";
const CHAT_PROTOCOL: &str = "live-tron.chat.v1";
const MAX_CHAT_CONNECTIONS_PER_STREAM: usize = 1000;
const CHAT_PRESENCE_LEASE: Duration = Duration::from_secs(45);
const CHAT_PRESENCE_HEARTBEAT: Duration = Duration::from_secs(15);

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
    let restricted = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM chat_restrictions WHERE stream_id = $1 AND user_id = $2 AND expires_at > now())",
    )
    .bind(stream_id)
    .bind(user_id)
    .fetch_one(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_restriction_check_failed"))?;
    if restricted {
        return Err(ApiError(StatusCode::FORBIDDEN, "chat_restricted"));
    }

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
    publish_message(&state, &message).await;
    Ok((StatusCode::CREATED, Json(message)))
}

#[derive(Serialize)]
pub struct ChatTicketResponse {
    ticket: String,
    expires_in: u64,
    websocket_protocol: &'static str,
}

pub async fn create_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(stream_id): Path<Uuid>,
) -> Result<Json<ChatTicketResponse>, ApiError> {
    let user_id = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    ensure_chat_access(db, stream_id, Some(user_id)).await?;
    let account_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
            .bind(user_id)
            .fetch_one(db)
            .await
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_ticket_failed"))?;
    if !account_exists {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "user_not_found"));
    }

    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let ticket = hex::encode(bytes);
    let ticket_hash = hash_chat_ticket(&ticket);
    sqlx::query("DELETE FROM chat_ws_tickets WHERE expires_at <= now()")
        .execute(db)
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "chat_ticket_cleanup_failed",
            )
        })?;
    sqlx::query(
        "INSERT INTO chat_ws_tickets (ticket_hash, stream_id, user_id, expires_at) VALUES ($1, $2, $3, now() + ($4::double precision * interval '1 second'))",
    )
    .bind(ticket_hash)
    .bind(stream_id)
    .bind(user_id)
    .bind(CHAT_TICKET_TTL.as_secs() as f64)
    .execute(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_ticket_failed"))?;
    Ok(Json(ChatTicketResponse {
        ticket,
        expires_in: CHAT_TICKET_TTL.as_secs(),
        websocket_protocol: CHAT_PROTOCOL,
    }))
}

fn chat_origin_allowed(headers: &HeaderMap, allowed_origins: &[String]) -> bool {
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| allowed_origins.iter().any(|allowed| allowed == origin))
}

pub async fn connect(
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    if !chat_origin_allowed(&headers, &state.allowed_origins) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "websocket_origin_not_allowed",
        ));
    }
    let ticket = headers
        .get(SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .find_map(|protocol| protocol.strip_prefix(CHAT_TICKET_PREFIX))
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "missing_or_invalid_chat_ticket",
        ))?;

    let db = pool(&state)?;
    // DELETE ... RETURNING makes ticket consumption atomic across every API replica.
    // Only the SHA-256 digest is stored; the bearer ticket is returned once to its creator.
    let ticket_hash = hash_chat_ticket(ticket);
    let record = sqlx::query_as::<_, (Uuid, Uuid)>(
        "DELETE FROM chat_ws_tickets WHERE ticket_hash = $1 AND expires_at > now() RETURNING stream_id, user_id",
    )
    .bind(ticket_hash)
    .fetch_optional(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_ticket_consume_failed"))?
    .ok_or(ApiError(
        StatusCode::UNAUTHORIZED,
        "chat_ticket_expired_or_used",
    ))?;
    let (ticket_stream_id, ticket_user_id) = record;
    if ticket_stream_id != stream_id {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "chat_ticket_expired_or_used",
        ));
    }
    let authorized = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM streams s JOIN channels c ON c.id = s.channel_id JOIN users u ON u.id = $2 WHERE s.id = $1 AND s.status = 'live' AND (s.visibility <> 'private' OR c.owner_user_id = $2))",
    )
    .bind(stream_id)
    .bind(ticket_user_id)
    .fetch_one(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_authorization_failed"))?;
    if !authorized {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "chat_ticket_access_revoked",
        ));
    }

    let (sender, receiver) = {
        let mut rooms = state.chat_rooms.lock().await;
        let sender = rooms.entry(stream_id).or_insert_with(|| {
            let (sender, _) = broadcast::channel(128);
            sender
        });
        if sender.receiver_count() >= MAX_CHAT_CONNECTIONS_PER_STREAM {
            return Err(ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "chat_room_capacity_reached",
            ));
        }
        (sender.clone(), sender.subscribe())
    };
    let state_for_socket = state.clone();
    let state_for_failure = state.clone();
    Ok(ws
        .protocols([CHAT_PROTOCOL])
        .max_message_size(4096)
        .max_frame_size(4096)
        .on_failed_upgrade(move |_| {
            tokio::spawn(async move {
                cleanup_chat_room(&state_for_failure, stream_id).await;
            });
        })
        .on_upgrade(move |socket| {
            handle_chat_socket(
                socket,
                state_for_socket,
                stream_id,
                ticket_user_id,
                sender,
                receiver,
            )
        }))
}

async fn handle_chat_socket(
    socket: WebSocket,
    state: AppState,
    stream_id: Uuid,
    user_id: Uuid,
    sender: broadcast::Sender<String>,
    mut receiver: broadcast::Receiver<String>,
) {
    let (mut outgoing, mut incoming) = socket.split();
    let connection_id = Uuid::new_v4();
    let Some(db) = state.db.as_ref() else {
        let _ = outgoing.send(Message::Close(None)).await;
        drop(receiver);
        drop(sender);
        cleanup_chat_room(&state, stream_id).await;
        return;
    };
    if let Err(error) = sqlx::query(
        "INSERT INTO chat_presence_leases (connection_id, stream_id, user_id, expires_at) VALUES ($1, $2, $3, now() + ($4::double precision * interval '1 second'))",
    )
    .bind(connection_id)
    .bind(stream_id)
    .bind(user_id)
    .bind(CHAT_PRESENCE_LEASE.as_secs() as f64)
    .execute(db)
    .await
    {
        tracing::error!(%stream_id, %error, "could not register chat presence lease");
        let _ = outgoing.send(Message::Close(None)).await;
        drop(receiver);
        drop(sender);
        cleanup_chat_room(&state, stream_id).await;
        return;
    }
    let participants = count_chat_participants(db, stream_id).await.unwrap_or(1);
    let ready = serde_json::json!({
        "type": "ready",
        "stream_id": stream_id,
        "user_id": user_id,
        "protocol": CHAT_PROTOCOL,
        "participants": participants
    });
    if outgoing
        .send(Message::Text(ready.to_string().into()))
        .await
        .is_err()
    {
        drop(receiver);
        drop(sender);
        cleanup_chat_room(&state, stream_id).await;
        return;
    }
    publish_chat_event(
        &state,
        stream_id,
        serde_json::json!({ "type": "presence", "participants": participants }),
    ).await;
    let mut presence_heartbeat = tokio::time::interval(CHAT_PRESENCE_HEARTBEAT);
    loop {
        tokio::select! {
            incoming_message = incoming.next() => {
                match incoming_message {
                    Some(Ok(Message::Ping(payload))) => {
                        if outgoing.send(Message::Pong(payload)).await.is_err() { break; }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Text(text))) if text.len() > 1024 => break,
                    Some(Ok(Message::Binary(bytes))) if bytes.len() > 1024 => break,
                    Some(Ok(_)) => {}
                }
            }
            _ = presence_heartbeat.tick() => {
                if let Some(db) = state.db.as_ref() {
                    if let Err(error) = sqlx::query(
                        "UPDATE chat_presence_leases SET expires_at = now() + ($2::double precision * interval '1 second') WHERE connection_id = $1",
                    )
                    .bind(connection_id)
                    .bind(CHAT_PRESENCE_LEASE.as_secs() as f64)
                    .execute(db)
                    .await
                    {
                        tracing::warn!(%stream_id, %error, "chat presence heartbeat failed");
                    }
                }
            }
            event = receiver.recv() => {
                match event {
                    Ok(payload) => {
                        if outgoing.send(Message::Text(payload.into())).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let warning = serde_json::json!({"type":"resync_required"});
                        if outgoing.send(Message::Text(warning.to_string().into())).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    drop(receiver);
    drop(sender);
    if let Some(db) = state.db.as_ref() {
        if let Err(error) = sqlx::query("DELETE FROM chat_presence_leases WHERE connection_id = $1")
            .bind(connection_id)
            .execute(db)
            .await
        {
            tracing::warn!(%stream_id, %error, "could not remove chat presence lease");
        } else if let Ok(participants) = count_chat_participants(db, stream_id).await {
            publish_chat_event(
                &state,
                stream_id,
                serde_json::json!({ "type": "presence", "participants": participants }),
            ).await;
        }
    }
    cleanup_chat_room(&state, stream_id).await;
}

async fn count_chat_participants(db: &PgPool, stream_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(DISTINCT user_id) FROM chat_presence_leases WHERE stream_id = $1 AND expires_at > now()",
    )
    .bind(stream_id)
    .fetch_one(db)
    .await
}

async fn cleanup_chat_room(state: &AppState, stream_id: Uuid) {
    let mut rooms = state.chat_rooms.lock().await;
    if let Some(sender) = rooms.get(&stream_id) {
        let participants = sender.receiver_count();
        if participants == 0 {
            rooms.remove(&stream_id);
        } else {
            let event = serde_json::json!({
                "type": "presence",
                "participants": participants
            });
            let _ = sender.send(event.to_string());
        }
    }
}

fn hash_chat_ticket(ticket: &str) -> String {
    hex::encode(Sha256::digest(ticket.as_bytes()))
}

async fn publish_chat_event(state: &AppState, stream_id: Uuid, event: serde_json::Value) {
    let Some(db) = state.db.as_ref() else {
        return;
    };
    let payload = serde_json::json!({
        "stream_id": stream_id,
        "event": event
    })
    .to_string();
    if let Err(error) = sqlx::query("SELECT pg_notify('live_tron_chat', $1)")
        .bind(payload)
        .execute(db)
        .await
    {
        // The message is already durable in PostgreSQL. Clients recover missed events
        // through the history endpoint after reconnect or a resync_required event.
        tracing::warn!(%stream_id, %error, "could not publish live chat notification");
    }
}

pub(crate) async fn publish_message(state: &AppState, message: &ChatMessage) {
    publish_chat_event(
        state,
        message.stream_id,
        serde_json::json!({
            "type": "message",
            "message": message
        }),
    )
    .await;
}

/// Start one PostgreSQL LISTEN loop per API process. Notifications are delivered to
/// every connected replica; each process forwards them only to its own local sockets.
pub fn start_chat_listener(state: AppState) {
    let Some(db) = state.db.clone() else {
        return;
    };
    tokio::spawn(async move {
        loop {
            let mut listener = match PgListener::connect_with(&db).await {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::error!(%error, "chat notification listener connection failed");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            if let Err(error) = listener.listen("live_tron_chat").await {
                tracing::error!(%error, "chat notification subscription failed");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            tracing::info!("subscribed to PostgreSQL live chat notifications");
            // Existing sockets may have missed events while this process was reconnecting.
            // Force clients to refresh durable history before consuming new notifications.
            {
                let rooms = state.chat_rooms.lock().await;
                let resync = serde_json::json!({"type":"resync_required"}).to_string();
                for sender in rooms.values() {
                    let _ = sender.send(resync.clone());
                }
            }
            loop {
                match listener.recv().await {
                    Ok(notification) => {
                        let Ok(value) =
                            serde_json::from_str::<serde_json::Value>(notification.payload())
                        else {
                            tracing::warn!("discarding malformed live chat notification");
                            continue;
                        };
                        let Some(stream_id) = value
                            .get("stream_id")
                            .and_then(|v| v.as_str())
                            .and_then(|v| Uuid::parse_str(v).ok())
                        else {
                            tracing::warn!(
                                "discarding chat notification without a valid stream ID"
                            );
                            continue;
                        };
                        let Some(event) = value.get("event") else {
                            tracing::warn!(%stream_id, "discarding chat notification without an event");
                            continue;
                        };
                        let payload = event.to_string();
                        let rooms = state.chat_rooms.lock().await;
                        if let Some(sender) = rooms.get(&stream_id) {
                            let _ = sender.send(payload);
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "chat notification listener disconnected; reconnecting");
                        break;
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

#[cfg(test)]
mod websocket_origin_tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn chat_ticket_storage_uses_a_fixed_length_one_way_digest() {
        let ticket = "a".repeat(64);
        let digest = hash_chat_ticket(&ticket);
        assert_eq!(digest.len(), 64);
        assert_ne!(digest, ticket);
        assert_eq!(digest, hash_chat_ticket(&ticket));
    }

    #[test]
    fn chat_websocket_requires_an_exact_allowlisted_origin() {
        let allowed = vec!["https://live.example.test".to_owned()];
        let mut headers = HeaderMap::new();
        assert!(!chat_origin_allowed(&headers, &allowed));

        headers.insert(ORIGIN, HeaderValue::from_static("not-an-origin"));
        assert!(!chat_origin_allowed(&headers, &allowed));

        headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://evil.example.test"),
        );
        assert!(!chat_origin_allowed(&headers, &allowed));

        headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://live.example.test"),
        );
        assert!(chat_origin_allowed(&headers, &allowed));
    }
}

#[derive(Deserialize)]
pub struct SetChatRestriction {
    kind: String,
    duration_seconds: i64,
    reason: Option<String>,
}

async fn ensure_stream_owner(db: &PgPool, stream_id: Uuid, user_id: Uuid) -> Result<(), ApiError> {
    let is_owner = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM streams s JOIN channels c ON c.id = s.channel_id WHERE s.id = $1 AND s.status = 'live' AND c.owner_user_id = $2)",
    )
    .bind(stream_id)
    .bind(user_id)
    .fetch_one(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_moderation_check_failed"))?;
    if !is_owner {
        return Err(ApiError(StatusCode::FORBIDDEN, "chat_moderation_forbidden"));
    }
    Ok(())
}

pub async fn set_restriction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((stream_id, user_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<SetChatRestriction>,
) -> Result<StatusCode, ApiError> {
    let actor_id = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    ensure_stream_owner(db, stream_id, actor_id).await?;
    if !matches!(input.kind.as_str(), "mute" | "ban")
        || !(1..=86_400).contains(&input.duration_seconds)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "invalid_chat_restriction",
        ));
    }
    let reason = input.reason.map(|value| value.trim().to_owned());
    if reason
        .as_ref()
        .is_some_and(|value| value.chars().count() > 240)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "chat_restriction_reason_too_long",
        ));
    }
    let target_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
            .bind(user_id)
            .fetch_one(db)
            .await
            .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_moderation_failed"))?;
    if !target_exists {
        return Err(ApiError(StatusCode::NOT_FOUND, "chat_user_not_found"));
    }
    sqlx::query(
        "INSERT INTO chat_restrictions (stream_id, user_id, created_by, kind, reason, expires_at) VALUES ($1, $2, $3, $4, $5, now() + ($6::double precision * interval '1 second')) ON CONFLICT (stream_id, user_id) DO UPDATE SET created_by = EXCLUDED.created_by, kind = EXCLUDED.kind, reason = EXCLUDED.reason, expires_at = EXCLUDED.expires_at, created_at = now()",
    )
    .bind(stream_id)
    .bind(user_id)
    .bind(actor_id)
    .bind(input.kind)
    .bind(reason)
    .bind(input.duration_seconds)
    .execute(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_moderation_failed"))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_restriction(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((stream_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let actor_id = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    ensure_stream_owner(db, stream_id, actor_id).await?;
    sqlx::query("DELETE FROM chat_restrictions WHERE stream_id = $1 AND user_id = $2")
        .bind(stream_id)
        .bind(user_id)
        .execute(db)
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_moderation_failed"))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((stream_id, message_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let actor_id = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    ensure_stream_owner(db, stream_id, actor_id).await?;
    let deleted = sqlx::query_scalar::<_, Uuid>(
        "DELETE FROM chat_messages m USING streams s, channels c WHERE m.id = $1 AND m.stream_id = $2 AND s.id = m.stream_id AND c.id = s.channel_id AND c.owner_user_id = $3 RETURNING m.id",
    )
    .bind(message_id)
    .bind(stream_id)
    .bind(actor_id)
    .fetch_optional(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "chat_message_delete_failed"))?;
    let Some(deleted_id) = deleted else {
        return Err(ApiError(StatusCode::NOT_FOUND, "chat_message_not_found"));
    };
    publish_message_deleted(&state, stream_id, deleted_id).await;
    Ok(StatusCode::NO_CONTENT)
}

async fn publish_message_deleted(state: &AppState, stream_id: Uuid, message_id: Uuid) {
    publish_chat_event(
        state,
        stream_id,
        serde_json::json!({
            "type": "message_deleted",
            "message_id": message_id
        }),
    )
    .await;
}
