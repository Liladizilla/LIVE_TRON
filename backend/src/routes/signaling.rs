use super::auth::{authenticated_user, ApiError};
use crate::state::{AppState, SignalRoom};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{
        header::{ORIGIN, SEC_WEBSOCKET_PROTOCOL},
        HeaderMap, StatusCode,
    },
    response::Response,
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgListener, PgPool};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
use uuid::Uuid;

const MAX_SIGNAL_BYTES: usize = 16 * 1024;
const TURN_CREDENTIAL_TTL_SECONDS: i64 = 3600;
const SIGNAL_TICKET_TTL_SECONDS: u64 = 60;
const MAX_ROOM_PARTICIPANTS: usize = 256;
const MAX_SIGNAL_MESSAGES_PER_SECOND: u32 = 60;
const SIGNAL_PEER_LEASE_SECONDS: i64 = 45;
const SIGNAL_PEER_HEARTBEAT_SECONDS: u64 = 15;
const SIGNAL_TICKET_PROTOCOL_PREFIX: &str = "live-tron.ticket.";
type HmacSha1 = Hmac<Sha1>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PeerRole {
    Publisher,
    Viewer,
}
impl PeerRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Publisher => "publisher",
            Self::Viewer => "viewer",
        }
    }
    fn is_publisher(self) -> bool {
        self == Self::Publisher
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientSignal {
    #[serde(rename = "type")]
    kind: String,
    to: Uuid,
    payload: Value,
}

#[derive(Serialize)]
pub struct IceServer {
    urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    credential: Option<String>,
}
#[derive(Serialize)]
pub struct IceConfig {
    ice_servers: Vec<IceServer>,
    expires_in: u64,
}

#[derive(Serialize)]
pub struct SignalTicketResponse {
    ticket: String,
    expires_in: u64,
    websocket_protocols: [String; 2],
}

pub async fn ice_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<IceConfig>, ApiError> {
    let user_id = authenticated_user(&headers, &state)?;
    let mut ice_servers = Vec::new();
    if !state.ice.stun_urls.is_empty() {
        ice_servers.push(IceServer {
            urls: state.ice.stun_urls.clone(),
            username: None,
            credential: None,
        });
    }
    if let (Some(secret), false) = (
        state.ice.turn_shared_secret.as_deref(),
        state.ice.turn_urls.is_empty(),
    ) {
        let (username, credential) =
            create_turn_credential(user_id, secret, Utc::now().timestamp())?;
        ice_servers.push(IceServer {
            urls: state.ice.turn_urls.clone(),
            username: Some(username),
            credential: Some(credential),
        });
    }
    Ok(Json(IceConfig {
        ice_servers,
        expires_in: TURN_CREDENTIAL_TTL_SECONDS as u64,
    }))
}

fn credential_hash(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

fn valid_publish_key(key: &str) -> bool {
    key.len() == 71
        && key.starts_with("lt_pub_")
        && key[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

async fn stream_access(
    db: &PgPool,
    stream_id: Uuid,
    headers: &HeaderMap,
    state: &AppState,
) -> Result<PeerRole, ApiError> {
    let row = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT c.owner_user_id, s.status, s.visibility FROM streams s JOIN channels c ON c.id = s.channel_id WHERE s.id = $1",
    )
    .bind(stream_id)
    .fetch_optional(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "signal_authorization_failed"))?
    .ok_or(ApiError(StatusCode::NOT_FOUND, "stream_not_found"))?;

    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();

    if let Some(key) = authorization.strip_prefix("Publish ") {
        if !valid_publish_key(key) || !matches!(row.1.as_str(), "created" | "live") {
            return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_publishing_key"));
        }
        let valid = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM stream_publish_credentials WHERE stream_id = $1 AND key_hash = $2 AND revoked_at IS NULL)",
        )
        .bind(stream_id)
        .bind(credential_hash(key))
        .fetch_one(db)
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "signal_authorization_failed"))?;
        if !valid {
            return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_publishing_key"));
        }
        return Ok(PeerRole::Publisher);
    }

    let viewer = authenticated_user(headers, state)?;
    if row.2 == "private" && viewer != row.0 {
        return Err(ApiError(StatusCode::NOT_FOUND, "stream_not_found"));
    }
    if row.1 != "live" {
        return Err(ApiError(StatusCode::CONFLICT, "stream_not_live"));
    }
    Ok(PeerRole::Viewer)
}

pub async fn create_ticket(
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<SignalTicketResponse>, ApiError> {
    let db = state.db.as_ref().ok_or(ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
    ))?;
    let role = stream_access(db, stream_id, &headers, &state).await?;
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let ticket = hex::encode(bytes);
    let ticket_hash = credential_hash(&ticket);
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let (publisher_key_hash, viewer_user_id) = if role.is_publisher() {
        let key = authorization
            .strip_prefix("Publish ")
            .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_publishing_key"))?;
        (Some(credential_hash(key)), None)
    } else {
        (None, Some(authenticated_user(&headers, &state)?))
    };
    sqlx::query("DELETE FROM signal_ws_tickets WHERE expires_at <= now()")
        .execute(db)
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "signal_ticket_cleanup_failed",
            )
        })?;
    sqlx::query(
        "INSERT INTO signal_ws_tickets (ticket_hash, stream_id, publisher, publisher_key_hash, viewer_user_id, expires_at) VALUES ($1, $2, $3, $4, $5, now() + ($6::double precision * interval '1 second'))",
    )
    .bind(ticket_hash)
    .bind(stream_id)
    .bind(role.is_publisher())
    .bind(publisher_key_hash)
    .bind(viewer_user_id)
    .bind(SIGNAL_TICKET_TTL_SECONDS as f64)
    .execute(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "signal_ticket_create_failed"))?;
    Ok(Json(SignalTicketResponse {
        ticket: ticket.clone(),
        expires_in: SIGNAL_TICKET_TTL_SECONDS,
        websocket_protocols: [
            "live-tron.signal.v1".to_owned(),
            format!("{SIGNAL_TICKET_PROTOCOL_PREFIX}{ticket}"),
        ],
    }))
}

pub async fn connect(
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    if !request_origin_is_allowed(&headers, &state.allowed_origins) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "websocket_origin_not_allowed",
        ));
    }
    let protocols = headers
        .get(SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let ticket = protocols
        .split(',')
        .map(str::trim)
        .find_map(|protocol| protocol.strip_prefix(SIGNAL_TICKET_PROTOCOL_PREFIX))
        .filter(|value| valid_signal_ticket(value))
        .ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "missing_or_invalid_signal_ticket",
        ))?;
    let db = state.db.as_ref().ok_or(ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
    ))?;
    // DELETE ... RETURNING consumes a one-use ticket atomically across API replicas.
    // Only its SHA-256 digest is stored; the bearer value is never persisted.
    let ticket_hash = credential_hash(ticket);
    let ticket_record = sqlx::query_as::<_, (Uuid, bool, Option<String>, Option<Uuid>)>(
        "DELETE FROM signal_ws_tickets WHERE ticket_hash = $1 AND expires_at > now() RETURNING stream_id, publisher, publisher_key_hash, viewer_user_id",
    )
    .bind(ticket_hash)
    .fetch_optional(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "signal_ticket_consume_failed"))?
    .ok_or(ApiError(
        StatusCode::UNAUTHORIZED,
        "signal_ticket_expired_or_used",
    ))?;
    if ticket_record.0 != stream_id {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "signal_ticket_expired_or_used",
        ));
    }
    let still_authorized = if ticket_record.1 {
        let Some(key_hash) = ticket_record.2.as_deref() else {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "signal_ticket_expired_or_used",
            ));
        };
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM streams s JOIN stream_publish_credentials c ON c.stream_id = s.id WHERE s.id = $1 AND s.status IN ('created', 'live') AND c.key_hash = $2 AND c.revoked_at IS NULL)",
        )
        .bind(stream_id)
        .bind(key_hash)
        .fetch_one(db)
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "signal_authorization_failed"))?
    } else {
        let Some(viewer_id) = ticket_record.3 else {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "signal_ticket_expired_or_used",
            ));
        };
        let stream = sqlx::query_as::<_, (Uuid, String, String)>(
            "SELECT c.owner_user_id, s.status, s.visibility FROM streams s JOIN channels c ON c.id = s.channel_id WHERE s.id = $1",
        )
        .bind(stream_id)
        .fetch_optional(db)
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "signal_authorization_failed"))?;
        let account_exists =
            sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM users WHERE id = $1)")
                .bind(viewer_id)
                .fetch_one(db)
                .await
                .map_err(|_| {
                    ApiError(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "signal_authorization_failed",
                    )
                })?;
        stream.is_some_and(|(owner_id, status, visibility)| {
            account_exists && status == "live" && (visibility != "private" || viewer_id == owner_id)
        })
    };
    if !still_authorized {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "signal_ticket_access_revoked",
        ));
    }
    let role = if ticket_record.1 {
        PeerRole::Publisher
    } else {
        PeerRole::Viewer
    };
    let participant_id = Uuid::new_v4();
    // A database lease makes peer presence visible across API replicas. The
    // partial unique index in migration 0009 guarantees one publisher per stream.
    sqlx::query("DELETE FROM signal_peer_leases WHERE expires_at <= now()")
        .execute(db)
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "signal_peer_cleanup_failed"))?;
    let lease_result = sqlx::query(
        "INSERT INTO signal_peer_leases (participant_id, stream_id, publisher, expires_at) VALUES ($1, $2, $3, now() + ($4::double precision * interval '1 second'))",
    )
    .bind(participant_id)
    .bind(stream_id)
    .bind(role.is_publisher())
    .bind(SIGNAL_PEER_LEASE_SECONDS as f64)
    .execute(db)
    .await;
    if let Err(error) = lease_result {
        if error.as_database_error().is_some_and(|db_error| db_error.code().as_deref() == Some("23505")) {
            return Err(ApiError(StatusCode::CONFLICT, "publisher_already_connected"));
        }
        return Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, "signal_peer_reservation_failed"));
    }
    let mut rooms = state.signal_rooms.lock().await;
    let room = rooms.entry(stream_id).or_insert_with(|| {
        let (sender, _) = broadcast::channel(64);
        SignalRoom {
            sender,
            publisher: None,
            participants: Default::default(),
        }
    });
    if role.is_publisher() && room.publisher.is_some() {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "publisher_already_connected",
        ));
    }
    if room.participants.len() >= MAX_ROOM_PARTICIPANTS {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "signal_room_capacity_reached",
        ));
    }
    // A newly connected peer must receive the existing room roster. Existing
    // peers will receive this participant through the normal peer_joined event.
    let existing_peers = peer_roster(&room.participants, role);
    let receiver = room.sender.subscribe();
    room.participants
        .insert(participant_id, role.is_publisher());
    if role.is_publisher() {
        room.publisher = Some(participant_id);
    }
    let sender = room.sender.clone();
    drop(rooms);

    let state_for_socket = state.clone();
    let state_for_failed_upgrade = state.clone();
    Ok(ws
        .protocols(["live-tron.signal.v1"])
        .max_message_size(MAX_SIGNAL_BYTES)
        .max_frame_size(MAX_SIGNAL_BYTES)
        .on_failed_upgrade(move |_| {
            // The participant is reserved before the HTTP upgrade response is sent.
            // If the handshake fails, release that reservation so a publisher slot
            // cannot remain stuck until the process restarts.
            tokio::spawn(async move {
                cleanup_participant(&state_for_failed_upgrade, stream_id, participant_id, role)
                    .await;
            });
        })
        .on_upgrade(move |socket| {
            handle_socket(
                socket,
                SocketContext {
                    state: state_for_socket,
                    stream_id,
                    participant_id,
                    role,
                    room_sender: sender,
                    room_receiver: receiver,
                    existing_peers,
                },
            )
        }))
}

struct SocketContext {
    state: AppState,
    stream_id: Uuid,
    participant_id: Uuid,
    role: PeerRole,
    room_sender: broadcast::Sender<String>,
    room_receiver: broadcast::Receiver<String>,
    existing_peers: Vec<(Uuid, bool)>,
}

async fn handle_socket(socket: WebSocket, context: SocketContext) {
    let SocketContext {
        state,
        stream_id,
        participant_id,
        role,
        room_sender,
        mut room_receiver,
        existing_peers,
    } = context;
    let (mut outgoing, mut incoming) = socket.split();
    let hello = json!({
        "type": "ready",
        "participant_id": participant_id,
        "stream_id": stream_id,
        "role": role.as_str(),
        "protocol": "live-tron.signal.v1",
        "media_transport": "not_connected"
    });
    if outgoing
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        cleanup_participant(&state, stream_id, participant_id, role).await;
        return;
    }
    // Send the existing roster directly to the new socket. Do not broadcast
    // these messages: they are only relevant to the participant joining now,
    // and a large roster must not overflow the bounded room broadcast channel.
    for (peer_id, is_publisher) in existing_peers {
        let peer_role = if is_publisher {
            PeerRole::Publisher
        } else {
            PeerRole::Viewer
        };
        let event = peer_joined_event(peer_id, peer_role);
        if outgoing
            .send(Message::Text(event.to_string().into()))
            .await
            .is_err()
        {
            cleanup_participant(&state, stream_id, participant_id, role).await;
            return;
        }
    }

    let mut signal_window = Instant::now();
    let mut signal_count = 0_u32;
    let joined = peer_joined_event(participant_id, role);
    let _ = room_sender.send(joined.to_string());
    publish_signal_event(&state, stream_id, joined).await;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(SIGNAL_PEER_HEARTBEAT_SECONDS));

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                let Some(db) = state.db.as_ref() else { break };
                let refreshed = sqlx::query(
                    "UPDATE signal_peer_leases SET expires_at = now() + ($2::double precision * interval '1 second') WHERE participant_id = $1",
                )
                .bind(participant_id)
                .bind(SIGNAL_PEER_LEASE_SECONDS as f64)
                .execute(db)
                .await;
                if refreshed.map(|result| result.rows_affected() == 0).unwrap_or(true) {
                    tracing::warn!(%stream_id, %participant_id, "signaling peer lease could not be refreshed");
                    break;
                }
            }
            incoming_message = incoming.next() => {
                let Some(Ok(message)) = incoming_message else { break };
                match message {
                    Message::Text(text) => {
                        if signal_window.elapsed() >= Duration::from_secs(1) {
                            signal_window = Instant::now();
                            signal_count = 0;
                        }
                        signal_count = signal_count.saturating_add(1);
                        if signal_count > MAX_SIGNAL_MESSAGES_PER_SECOND {
                            let _ = outgoing.send(Message::Text(json!({"type":"error","error":"signal_rate_limit_exceeded"}).to_string().into())).await;
                            continue;
                        }
                        if text.len() > MAX_SIGNAL_BYTES {
                            let _ = outgoing.send(Message::Close(None)).await;
                            break;
                        }
                        let Ok(signal) = serde_json::from_str::<ClientSignal>(&text) else {
                            let _ = outgoing.send(Message::Text(json!({"type":"error","error":"invalid_signal_message"}).to_string().into())).await;
                            continue;
                        };
                        if !valid_signal_kind(&signal.kind, role)
                            || serde_json::to_vec(&signal.payload).map_or(true, |bytes| bytes.len() > MAX_SIGNAL_BYTES / 2)
                        {
                            let _ = outgoing.send(Message::Text(json!({"type":"error","error":"signal_type_or_payload_rejected"}).to_string().into())).await;
                            continue;
                        }
                        let target_is_valid = sqlx::query_scalar::<_, bool>(
                            "SELECT EXISTS(SELECT 1 FROM signal_peer_leases WHERE participant_id = $1 AND stream_id = $2 AND expires_at > now() AND publisher <> $3)",
                        )
                        .bind(signal.to)
                        .bind(stream_id)
                        .bind(role.is_publisher())
                        .fetch_one(state.db.as_ref().expect("signaling requires a database"))
                        .await
                        .unwrap_or(false);
                        if !target_is_valid || signal.to == participant_id {
                            let _ = outgoing.send(Message::Text(json!({"type":"error","error":"invalid_signal_recipient"}).to_string().into())).await;
                            continue;
                        }
                        let envelope = json!({
                            "type": signal.kind,
                            "from": participant_id,
                            "to": signal.to,
                            "role": role.as_str(),
                            "payload": signal.payload
                        });
                        let payload = envelope.to_string();
                        let _ = room_sender.send(payload.clone());
                        publish_signal_event(&state, stream_id, envelope).await;
                    }
                    Message::Ping(payload) => {
                        if outgoing.send(Message::Pong(payload)).await.is_err() { break; }
                    }
                    Message::Close(_) => break,
                    Message::Binary(_) | Message::Pong(_) => {}
                }
            }
            outbound = room_receiver.recv() => {
                match outbound {
                    Ok(payload) => {
                        let event = serde_json::from_str::<Value>(&payload).ok();
                        let should_deliver = event.as_ref().is_some_and(|value| {
                            value.get("to").and_then(Value::as_str)
                                .is_some_and(|target| target == participant_id.to_string())
                                || (value.get("to").is_none()
                                    && matches!(value.get("type").and_then(Value::as_str), Some("peer_joined" | "peer_left"))
                                    && value.get("from").and_then(Value::as_str)
                                        .is_some_and(|sender| sender != participant_id.to_string()))
                        });
                        if should_deliver
                            && outgoing.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }
    let left = json!({
        "type": "peer_left",
        "from": participant_id,
        "role": role.as_str()
    });
    let _ = room_sender.send(left.to_string());
    publish_signal_event(&state, stream_id, left).await;
    cleanup_participant(&state, stream_id, participant_id, role).await;
}

async fn publish_signal_event(state: &AppState, stream_id: Uuid, event: Value) {
    let Some(db) = state.db.as_ref() else { return };
    let payload = json!({"stream_id": stream_id, "event": event}).to_string();
    if let Err(error) = sqlx::query("SELECT pg_notify('live_tron_signal', $1)")
        .bind(payload)
        .execute(db)
        .await
    {
        tracing::warn!(%stream_id, %error, "could not publish signaling notification");
    }
}

/// Every API replica subscribes to the same signaling channel and forwards
/// notifications only to sockets connected to that replica.
pub fn start_signal_listener(state: AppState) {
    let Some(db) = state.db.clone() else { return };
    tokio::spawn(async move {
        loop {
            let mut listener = match PgListener::connect_with(&db).await {
                Ok(listener) => listener,
                Err(error) => {
                    tracing::error!(%error, "signaling listener connection failed");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            if let Err(error) = listener.listen("live_tron_signal").await {
                tracing::error!(%error, "signaling listener subscription failed");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            tracing::info!("subscribed to PostgreSQL signaling notifications");
            {
                let rooms = state.signal_rooms.lock().await;
                for (stream_id, room) in rooms.iter() {
                    let _ = room.sender.send(json!({
                        "type": "resync_required",
                        "stream_id": stream_id
                    }).to_string());
                }
            }
            loop {
                match listener.recv().await {
                    Ok(notification) => {
                        let Ok(value) = serde_json::from_str::<Value>(notification.payload()) else {
                            tracing::warn!("discarding malformed signaling notification");
                            continue;
                        };
                        let Some(stream_id) = value.get("stream_id").and_then(Value::as_str)
                            .and_then(|value| Uuid::parse_str(value).ok()) else { continue };
                        let Some(event) = value.get("event") else { continue };
                        let rooms = state.signal_rooms.lock().await;
                        if let Some(room) = rooms.get(&stream_id) {
                            let _ = room.sender.send(event.to_string());
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "signaling listener disconnected; reconnecting");
                        break;
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

fn create_turn_credential(
    user_id: Uuid,
    secret: &str,
    now: i64,
) -> Result<(String, String), ApiError> {
    let expires_at = now + TURN_CREDENTIAL_TTL_SECONDS;
    let username = format!("{expires_at}:{user_id}");
    let mut mac = HmacSha1::new_from_slice(secret.as_bytes())
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "turn_credentials_failed"))?;
    mac.update(username.as_bytes());
    Ok((username, STANDARD.encode(mac.finalize().into_bytes())))
}

fn peer_roster(participants: &HashMap<Uuid, bool>, joining_role: PeerRole) -> Vec<(Uuid, bool)> {
    participants
        .iter()
        .filter_map(|(id, is_publisher)| {
            (*is_publisher != joining_role.is_publisher()).then_some((*id, *is_publisher))
        })
        .collect()
}

fn peer_joined_event(participant_id: Uuid, role: PeerRole) -> Value {
    json!({
        "type": "peer_joined",
        "from": participant_id,
        "role": role.as_str()
    })
}

fn valid_signal_ticket(ticket: &str) -> bool {
    ticket.len() == 64 && ticket.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn origin_is_allowed(origin: &str, allowed_origins: &[String]) -> bool {
    allowed_origins.iter().any(|allowed| allowed == origin)
}

// Browser WebSocket requests must carry an Origin header. Treat a missing,
// malformed, or unlisted origin as a denial instead of silently allowing it.
fn request_origin_is_allowed(headers: &HeaderMap, allowed_origins: &[String]) -> bool {
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin_is_allowed(origin, allowed_origins))
}

fn valid_signal_kind(kind: &str, role: PeerRole) -> bool {
    match kind {
        "offer" | "renegotiate" => role == PeerRole::Publisher,
        "answer" => role == PeerRole::Viewer,
        "ice_candidate" => true,
        _ => false,
    }
}

async fn cleanup_participant(
    state: &AppState,
    stream_id: Uuid,
    participant_id: Uuid,
    role: PeerRole,
) {
    if let Some(db) = state.db.as_ref() {
        if let Err(error) = sqlx::query("DELETE FROM signal_peer_leases WHERE participant_id = $1")
            .bind(participant_id)
            .execute(db)
            .await
        {
            tracing::warn!(%stream_id, %participant_id, %error, "could not release signaling peer lease");
        }
    }
    let mut rooms = state.signal_rooms.lock().await;
    if let Some(room) = rooms.get_mut(&stream_id) {
        room.participants.remove(&participant_id);
        if role.is_publisher() && room.publisher == Some(participant_id) {
            room.publisher = None;
        }
        if room.participants.is_empty() {
            rooms.remove(&stream_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        create_turn_credential, origin_is_allowed, valid_publish_key, valid_signal_kind,
        valid_signal_ticket, PeerRole,
    };
    use uuid::Uuid;

    #[test]
    fn signaling_roles_cannot_invert_offer_answer_flow() {
        assert!(valid_signal_kind("offer", PeerRole::Publisher));
        assert!(valid_signal_kind("renegotiate", PeerRole::Publisher));
        assert!(valid_signal_kind("answer", PeerRole::Viewer));
        assert!(valid_signal_kind("ice_candidate", PeerRole::Viewer));
        assert!(!valid_signal_kind("offer", PeerRole::Viewer));
        assert!(!valid_signal_kind("answer", PeerRole::Publisher));
        assert!(!valid_signal_kind("arbitrary", PeerRole::Publisher));
    }

    #[test]
    fn publishing_key_format_is_strict() {
        assert!(valid_publish_key(&format!("lt_pub_{}", "a".repeat(64))));
        assert!(!valid_publish_key("lt_pub_short"));
        assert!(!valid_publish_key(&format!("lt_pub_{}", "g".repeat(64))));
    }

    #[test]
    fn turn_credentials_are_ephemeral_and_bound_to_user_identity() {
        let user_id = Uuid::new_v4();
        let secret = "a-long-turn-rest-shared-secret-for-tests";
        let (username, credential) =
            create_turn_credential(user_id, secret, 1_800_000_000).unwrap();
        assert_eq!(username, format!("1800003600:{user_id}"));
        assert!(!credential.is_empty());
        assert_ne!(credential, secret);
        let (_, other_user_credential) =
            create_turn_credential(Uuid::new_v4(), secret, 1_800_000_000).unwrap();
        assert_ne!(credential, other_user_credential);
    }

    #[test]
    fn signaling_tickets_are_fixed_length_hex_capabilities() {
        assert!(valid_signal_ticket(&"a".repeat(64)));
        assert!(!valid_signal_ticket("short"));
        assert!(!valid_signal_ticket(&"g".repeat(64)));
    }

    #[test]
    fn peer_roster_events_identify_publisher_and_viewer_roles() {
        let publisher_id = Uuid::new_v4();
        let viewer_id = Uuid::new_v4();
        let publisher = super::peer_joined_event(publisher_id, PeerRole::Publisher);
        let viewer = super::peer_joined_event(viewer_id, PeerRole::Viewer);

        assert_eq!(publisher["type"], "peer_joined");
        assert_eq!(publisher["from"], publisher_id.to_string());
        assert_eq!(publisher["role"], "publisher");
        assert_eq!(viewer["from"], viewer_id.to_string());
        assert_eq!(viewer["role"], "viewer");
    }

    #[test]
    fn peer_roster_only_includes_the_opposite_signaling_role() {
        use std::collections::HashMap;

        let publisher_id = Uuid::new_v4();
        let viewer_one_id = Uuid::new_v4();
        let viewer_two_id = Uuid::new_v4();
        let mut participants = HashMap::new();
        participants.insert(publisher_id, true);
        participants.insert(viewer_one_id, false);
        participants.insert(viewer_two_id, false);

        let publisher_roster = super::peer_roster(&participants, PeerRole::Publisher);
        let viewer_roster = super::peer_roster(&participants, PeerRole::Viewer);

        assert_eq!(publisher_roster.len(), 2);
        assert!(publisher_roster
            .iter()
            .all(|(_, is_publisher)| !*is_publisher));
        assert_eq!(viewer_roster, vec![(publisher_id, true)]);
    }

    #[test]
    fn websocket_origins_must_match_an_exact_allowlist_entry() {
        let allowed = vec!["https://live.example".to_owned()];
        assert!(origin_is_allowed("https://live.example", &allowed));
        assert!(!origin_is_allowed("https://evil.example", &allowed));
        assert!(!origin_is_allowed(
            "https://live.example.evil.test",
            &allowed
        ));
        assert!(!origin_is_allowed("https://live.example", &[]));
    }

    #[test]
    fn websocket_requests_without_origin_are_rejected() {
        use axum::http::HeaderMap;
        let headers = HeaderMap::new();
        let allowed = vec!["https://live.example".to_owned()];
        assert!(!super::request_origin_is_allowed(&headers, &allowed));
    }

    #[test]
    fn websocket_requests_with_invalid_origin_encoding_are_rejected() {
        use axum::http::{header::ORIGIN, HeaderMap, HeaderValue};
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, HeaderValue::from_bytes(&[0xff]).unwrap());
        let allowed = vec!["https://live.example".to_owned()];
        assert!(!super::request_origin_is_allowed(&headers, &allowed));
    }

    #[test]
    fn websocket_requests_with_unlisted_origin_are_rejected() {
        use axum::http::{header::ORIGIN, HeaderMap};
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "https://attacker.example".parse().unwrap());
        let allowed = vec!["https://live.example".to_owned()];
        assert!(!super::request_origin_is_allowed(&headers, &allowed));
    }

    #[test]
    fn websocket_requests_with_allowlisted_origin_are_accepted() {
        use axum::http::{header::ORIGIN, HeaderMap};
        let mut headers = HeaderMap::new();
        headers.insert(ORIGIN, "https://live.example".parse().unwrap());
        let allowed = vec!["https://live.example".to_owned()];
        assert!(super::request_origin_is_allowed(&headers, &allowed));
    }

    #[tokio::test]
    async fn failed_upgrade_cleanup_releases_publisher_reservation() {
        use crate::state::{AppState, SignalRoom};
        use std::collections::HashMap;
        use tokio::sync::broadcast;
        use uuid::Uuid;

        let state = AppState::default();
        let stream_id = Uuid::new_v4();
        let participant_id = Uuid::new_v4();
        let (sender, _) = broadcast::channel(8);
        {
            let mut rooms = state.signal_rooms.lock().await;
            let mut participants = HashMap::new();
            participants.insert(participant_id, true);
            rooms.insert(
                stream_id,
                SignalRoom {
                    sender,
                    publisher: Some(participant_id),
                    participants,
                },
            );
        }

        super::cleanup_participant(
            &state,
            stream_id,
            participant_id,
            super::PeerRole::Publisher,
        )
        .await;

        let rooms = state.signal_rooms.lock().await;
        assert!(!rooms.contains_key(&stream_id));
    }

    #[tokio::test]
    async fn viewer_cleanup_preserves_active_publisher() {
        use crate::state::{AppState, SignalRoom};
        use std::collections::HashMap;
        use tokio::sync::broadcast;
        use uuid::Uuid;

        let state = AppState::default();
        let stream_id = Uuid::new_v4();
        let publisher_id = Uuid::new_v4();
        let viewer_id = Uuid::new_v4();
        let (sender, _) = broadcast::channel(8);
        {
            let mut rooms = state.signal_rooms.lock().await;
            let mut participants = HashMap::new();
            participants.insert(publisher_id, true);
            participants.insert(viewer_id, false);
            rooms.insert(
                stream_id,
                SignalRoom {
                    sender,
                    publisher: Some(publisher_id),
                    participants,
                },
            );
        }

        super::cleanup_participant(&state, stream_id, viewer_id, super::PeerRole::Viewer).await;

        let rooms = state.signal_rooms.lock().await;
        let room = rooms.get(&stream_id).expect("publisher room must remain");
        assert_eq!(room.publisher, Some(publisher_id));
        assert!(room.participants.contains_key(&publisher_id));
        assert!(!room.participants.contains_key(&viewer_id));
    }

    #[tokio::test]
    async fn publisher_cleanup_does_not_remove_other_viewers() {
        use crate::state::{AppState, SignalRoom};
        use std::collections::HashMap;
        use tokio::sync::broadcast;
        use uuid::Uuid;

        let state = AppState::default();
        let stream_id = Uuid::new_v4();
        let publisher_id = Uuid::new_v4();
        let viewer_id = Uuid::new_v4();
        let (sender, _) = broadcast::channel(8);
        {
            let mut rooms = state.signal_rooms.lock().await;
            let mut participants = HashMap::new();
            participants.insert(publisher_id, true);
            participants.insert(viewer_id, false);
            rooms.insert(
                stream_id,
                SignalRoom {
                    sender,
                    publisher: Some(publisher_id),
                    participants,
                },
            );
        }

        super::cleanup_participant(&state, stream_id, publisher_id, super::PeerRole::Publisher)
            .await;

        let rooms = state.signal_rooms.lock().await;
        let room = rooms.get(&stream_id).expect("viewer room must remain");
        assert_eq!(room.publisher, None);
        assert!(room.participants.contains_key(&viewer_id));
    }
}
