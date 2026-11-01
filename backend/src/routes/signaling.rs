use super::auth::{authenticated_user, ApiError};
use crate::state::{AppState, SignalRoom, SignalTicket};
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
use sqlx::PgPool;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use uuid::Uuid;

const MAX_SIGNAL_BYTES: usize = 16 * 1024;
const TURN_CREDENTIAL_TTL_SECONDS: i64 = 3600;
const SIGNAL_TICKET_TTL_SECONDS: u64 = 60;
const MAX_SIGNAL_TICKETS: usize = 10_000;
const MAX_ROOM_PARTICIPANTS: usize = 256;
const MAX_SIGNAL_MESSAGES_PER_SECOND: u32 = 60;
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
    let mut tickets = state.signal_tickets.lock().await;
    tickets.retain(|_, existing| existing.expires_at > Instant::now());
    if tickets.len() >= MAX_SIGNAL_TICKETS {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "signal_ticket_capacity_reached",
        ));
    }
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
    tickets.insert(
        ticket_hash,
        SignalTicket {
            stream_id,
            publisher: role.is_publisher(),
            publisher_key_hash,
            viewer_user_id,
            expires_at: Instant::now() + Duration::from_secs(SIGNAL_TICKET_TTL_SECONDS),
        },
    );
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
    if let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) {
        if !origin_is_allowed(origin, &state.allowed_origins) {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "websocket_origin_not_allowed",
            ));
        }
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
    let ticket_hash = credential_hash(ticket);
    let ticket_record = state
        .signal_tickets
        .lock()
        .await
        .remove(&ticket_hash)
        .ok_or(ApiError(
            StatusCode::UNAUTHORIZED,
            "signal_ticket_expired_or_used",
        ))?;
    if ticket_record.stream_id != stream_id || ticket_record.expires_at <= Instant::now() {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "signal_ticket_expired_or_used",
        ));
    }
    let db = state.db.as_ref().ok_or(ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
    ))?;
    let still_authorized = if ticket_record.publisher {
        let Some(key_hash) = ticket_record.publisher_key_hash.as_deref() else {
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
        let Some(viewer_id) = ticket_record.viewer_user_id else {
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
    let role = if ticket_record.publisher {
        PeerRole::Publisher
    } else {
        PeerRole::Viewer
    };
    let participant_id = Uuid::new_v4();
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
    let receiver = room.sender.subscribe();
    room.participants
        .insert(participant_id, role.is_publisher());
    if role.is_publisher() {
        room.publisher = Some(participant_id);
    }
    let sender = room.sender.clone();
    drop(rooms);

    let state_for_socket = state.clone();
    Ok(ws
        .protocols(["live-tron.signal.v1"])
        .max_message_size(MAX_SIGNAL_BYTES)
        .max_frame_size(MAX_SIGNAL_BYTES)
        .on_upgrade(move |socket| {
            handle_socket(
                socket,
                state_for_socket,
                stream_id,
                participant_id,
                role,
                sender,
                receiver,
            )
        }))
}

async fn handle_socket(
    socket: WebSocket,
    state: AppState,
    stream_id: Uuid,
    participant_id: Uuid,
    role: PeerRole,
    room_sender: broadcast::Sender<String>,
    mut room_receiver: broadcast::Receiver<String>,
) {
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
    let mut signal_window = Instant::now();
    let mut signal_count = 0_u32;
    let joined = json!({
        "type": "peer_joined",
        "from": participant_id,
        "role": role.as_str()
    });
    let _ = room_sender.send(joined.to_string());

    loop {
        tokio::select! {
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
                        let target_is_valid = {
                            let rooms = state.signal_rooms.lock().await;
                            rooms.get(&stream_id).and_then(|room| room.participants.get(&signal.to))
                                .is_some_and(|target_is_publisher| *target_is_publisher != role.is_publisher())
                        };
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
                        let _ = room_sender.send(envelope.to_string());
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
    cleanup_participant(&state, stream_id, participant_id, role).await;
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

fn valid_signal_ticket(ticket: &str) -> bool {
    ticket.len() == 64 && ticket.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn origin_is_allowed(origin: &str, allowed_origins: &[String]) -> bool {
    allowed_origins.iter().any(|allowed| allowed == origin)
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
}
