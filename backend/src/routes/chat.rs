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
use sqlx::{FromRow, PgPool};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use uuid::Uuid;

const CHAT_TICKET_TTL: Duration = Duration::from_secs(60);
const CHAT_TICKET_PREFIX: &str = "live-tron.chat.ticket.";
const CHAT_PROTOCOL: &str = "live-tron.chat.v1";
const MAX_CHAT_CONNECTIONS_PER_STREAM: usize = 1000;

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
    let mut tickets = state.chat_tickets.lock().await;
    tickets.retain(|_, value| value.expires_at > Instant::now());
    if tickets.len() >= 10_000 {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "chat_ticket_capacity_reached",
        ));
    }
    tickets.insert(
        ticket.clone(),
        crate::state::ChatTicket {
            stream_id,
            user_id,
            expires_at: Instant::now() + CHAT_TICKET_TTL,
        },
    );
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

    let record = state
        .chat_tickets
        .lock()
        .await
        .remove(ticket)
        .ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "chat_ticket_expired_or_used",
        ))?;
    if record.stream_id != stream_id || record.expires_at <= Instant::now() {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "chat_ticket_expired_or_used",
        ));
    }
    let db = pool(&state)?;
    let authorized = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM streams s JOIN channels c ON c.id = s.channel_id JOIN users u ON u.id = $2 WHERE s.id = $1 AND s.status = 'live' AND (s.visibility <> 'private' OR c.owner_user_id = $2))",
    )
    .bind(stream_id)
    .bind(record.user_id)
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
                record.user_id,
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
    let ready = serde_json::json!({
        "type": "ready",
        "stream_id": stream_id,
        "user_id": user_id,
        "protocol": CHAT_PROTOCOL,
        "participants": sender.receiver_count()
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
    let joined = serde_json::json!({
        "type": "presence",
        "participants": sender.receiver_count()
    });
    let _ = sender.send(joined.to_string());
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
    cleanup_chat_room(&state, stream_id).await;
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

pub(crate) async fn publish_message(state: &AppState, message: &ChatMessage) {
    let Ok(payload) = serde_json::to_string(&serde_json::json!({
        "type": "message",
        "message": message
    })) else {
        return;
    };
    let rooms = state.chat_rooms.lock().await;
    if let Some(sender) = rooms.get(&message.stream_id) {
        let _ = sender.send(payload);
    }
}


#[cfg(test)]
mod websocket_origin_tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn chat_websocket_requires_an_exact_allowlisted_origin() {
        let allowed = vec!["https://live.example.test".to_owned()];
        let mut headers = HeaderMap::new();
        assert!(!chat_origin_allowed(&headers, &allowed));

        headers.insert(ORIGIN, HeaderValue::from_static("not-an-origin"));
        assert!(!chat_origin_allowed(&headers, &allowed));

        headers.insert(ORIGIN, HeaderValue::from_static("https://evil.example.test"));
        assert!(!chat_origin_allowed(&headers, &allowed));

        headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://live.example.test"),
        );
        assert!(chat_origin_allowed(&headers, &allowed));
    }
}
