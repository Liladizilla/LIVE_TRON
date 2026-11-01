use super::auth::{authenticated_user, ApiError};
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;
const MEDIA_READ_TOKEN_TTL_SECONDS: i64 = 300;

/// MediaMTX HTTP-auth request. MediaMTX sends this payload for each publish/read
/// authorization attempt. Unknown fields are ignored for forward compatibility.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAuthRequest {
    #[serde(default)]
    action: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    token: String,
}

fn credential_hash(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

fn parse_stream_path(path: &str) -> Option<Uuid> {
    let id = Uuid::parse_str(path).ok()?;
    // Require the canonical UUID representation; do not accept alternate path forms.
    (id.to_string() == path).then_some(id)
}

fn valid_publish_key(key: &str) -> bool {
    key.len() == 71
        && key.starts_with("lt_pub_")
        && key[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn create_read_token(
    stream_id: Uuid,
    user_id: Uuid,
    secret: &str,
    now: i64,
) -> Result<String, ApiError> {
    let expires_at = now + MEDIA_READ_TOKEN_TTL_SECONDS;
    let payload = format!("v1|{stream_id}|{user_id}|{expires_at}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "media_token_signing_failed",
        )
    })?;
    mac.update(payload.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    Ok(format!("{}.{}", URL_SAFE_NO_PAD.encode(payload), signature))
}

fn verify_read_token(token: &str, stream_id: Uuid, secret: &str, now: i64) -> Option<Uuid> {
    let (encoded_payload, supplied_signature) = token.split_once('.')?;
    let payload = URL_SAFE_NO_PAD.decode(encoded_payload).ok()?;
    let payload_text = std::str::from_utf8(&payload).ok()?;
    let mut fields = payload_text.split('|');
    if fields.next()? != "v1" {
        return None;
    }
    let token_stream_id = Uuid::parse_str(fields.next()?).ok()?;
    let user_id = Uuid::parse_str(fields.next()?).ok()?;
    let expires_at = fields.next()?.parse::<i64>().ok()?;
    if fields.next().is_some() || token_stream_id != stream_id || expires_at <= now {
        return None;
    }
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(payload_text.as_bytes());
    let signature = hex::decode(supplied_signature).ok()?;
    mac.verify_slice(&signature).ok()?;
    Some(user_id)
}

#[derive(serde::Serialize)]
pub struct MediaReadTokenResponse {
    token: String,
    expires_in: u64,
}

pub async fn create_media_read_token(
    State(state): State<AppState>,
    Path(stream_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<MediaReadTokenResponse>, ApiError> {
    let user_id = authenticated_user(&headers, &state)?;
    let db = state.db.as_ref().ok_or(ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
    ))?;
    let row = sqlx::query_as::<_, (Uuid, String, bool)>(
        "SELECT c.owner_user_id, s.status, c.is_mature FROM streams s JOIN channels c ON c.id = s.channel_id WHERE s.id = $1",
    )
    .bind(stream_id)
    .fetch_optional(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "media_token_authorization_failed"))?
    .ok_or(ApiError(StatusCode::NOT_FOUND, "stream_not_found"))?;
    if row.1 != "live" || row.2 {
        return Err(ApiError(StatusCode::NOT_FOUND, "stream_not_found"));
    }
    let visibility =
        sqlx::query_scalar::<_, String>("SELECT visibility FROM streams WHERE id = $1")
            .bind(stream_id)
            .fetch_one(db)
            .await
            .map_err(|_| {
                ApiError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "media_token_authorization_failed",
                )
            })?;
    if visibility == "private" && user_id != row.0 {
        return Err(ApiError(StatusCode::NOT_FOUND, "stream_not_found"));
    }
    let secret = state.mediamtx_auth_secret.as_deref().ok_or(ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "media_authorization_not_configured",
    ))?;
    let token = create_read_token(stream_id, user_id, secret, Utc::now().timestamp())?;
    Ok(Json(MediaReadTokenResponse {
        token,
        expires_in: MEDIA_READ_TOKEN_TTL_SECONDS as u64,
    }))
}

async fn authorize(
    db: &PgPool,
    input: &MediaAuthRequest,
    secret: &str,
) -> Result<bool, sqlx::Error> {
    let Some(stream_id) = parse_stream_path(&input.path) else {
        return Ok(false);
    };

    match input.action.as_str() {
        "publish" => {
            if !valid_publish_key(&input.token) {
                return Ok(false);
            }
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(
                    SELECT 1
                    FROM streams s
                    JOIN stream_publish_credentials p ON p.stream_id = s.id
                    WHERE s.id = $1
                      AND s.status IN ('created', 'live')
                      AND p.key_hash = $2
                      AND p.revoked_at IS NULL
                )",
            )
            .bind(stream_id)
            .bind(credential_hash(&input.token))
            .fetch_one(db)
            .await
        }
        "read" => {
            if input.token.is_empty() {
                return sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(
                        SELECT 1 FROM streams s
                        JOIN channels c ON c.id = s.channel_id
                        WHERE s.id = $1 AND s.status = 'live'
                          AND s.visibility IN ('public', 'unlisted')
                          AND c.is_mature = false
                    )",
                )
                .bind(stream_id)
                .fetch_one(db)
                .await;
            }
            let Some(user_id) =
                verify_read_token(&input.token, stream_id, secret, Utc::now().timestamp())
            else {
                return Ok(false);
            };
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(
                    SELECT 1 FROM streams s
                    JOIN channels c ON c.id = s.channel_id
                    JOIN users viewer ON viewer.id = $2
                    WHERE s.id = $1 AND s.status = 'live'
                      AND c.is_mature = false
                      AND (s.visibility <> 'private' OR c.owner_user_id = viewer.id)
                )",
            )
            .bind(stream_id)
            .bind(user_id)
            .fetch_one(db)
            .await
        }
        _ => Ok(false),
    }
}

#[derive(Deserialize)]
pub struct MediaAuthQuery {
    secret: String,
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

/// MediaMTX authHTTP callback. The shared secret is supplied as a query parameter
/// because MediaMTX's HTTP-auth contract does not support custom request headers.
pub async fn authorize_media(
    State(state): State<AppState>,
    Query(query): Query<MediaAuthQuery>,
    Json(input): Json<MediaAuthRequest>,
) -> StatusCode {
    let Some(secret) = state.mediamtx_auth_secret.as_deref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    if !constant_time_equal(secret.as_bytes(), query.secret.as_bytes()) {
        return StatusCode::UNAUTHORIZED;
    }
    let Some(db) = state.db.as_ref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };

    match authorize(db, &input, secret).await {
        Ok(true) => StatusCode::OK,
        Ok(false) => StatusCode::UNAUTHORIZED,
        Err(error) => {
            tracing::error!(error = %error, "MediaMTX authorization query failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        constant_time_equal, create_read_token, parse_stream_path, valid_publish_key,
        verify_read_token,
    };
    use uuid::Uuid;

    #[test]
    fn media_callback_secret_comparison_is_constant_time_and_length_sensitive() {
        assert!(constant_time_equal(b"same-secret", b"same-secret"));
        assert!(!constant_time_equal(b"same-secret", b"wrong-secret"));
        assert!(!constant_time_equal(b"short", b"longer"));
    }

    #[test]
    fn media_paths_require_canonical_stream_uuids() {
        let id = Uuid::new_v4();
        assert_eq!(parse_stream_path(&id.to_string()), Some(id));
        assert_eq!(parse_stream_path("not-a-uuid"), None);
        assert_eq!(parse_stream_path(&id.to_string().to_uppercase()), None);
        assert_eq!(parse_stream_path(&format!("{id}/extra")), None);
    }

    #[test]
    fn media_read_tokens_are_bound_to_stream_user_and_expiry() {
        let stream_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let secret = "a-long-mediamtx-shared-secret-for-tests";
        let token = create_read_token(stream_id, user_id, secret, 1_800_000_000).unwrap();
        assert_eq!(
            verify_read_token(&token, stream_id, secret, 1_800_000_001),
            Some(user_id)
        );
        assert_eq!(
            verify_read_token(&token, Uuid::new_v4(), secret, 1_800_000_001),
            None
        );
        assert_eq!(
            verify_read_token(&token, stream_id, "wrong-secret", 1_800_000_001),
            None
        );
        assert_eq!(
            verify_read_token(&token, stream_id, secret, 1_800_000_301),
            None
        );
    }

    #[test]
    fn media_publish_keys_are_strictly_validated() {
        assert!(valid_publish_key(&format!("lt_pub_{}", "a".repeat(64))));
        assert!(!valid_publish_key("lt_pub_short"));
        assert!(!valid_publish_key(&format!("lt_pub_{}", "z".repeat(64))));
    }
}
