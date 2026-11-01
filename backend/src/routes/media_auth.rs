use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

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

async fn authorize(
    db: &PgPool,
    input: &MediaAuthRequest,
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
            // This first media-server integration deliberately permits only public,
            // non-mature live streams. Private/unlisted viewer grants need a separate
            // short-lived signed media token flow before they can be enabled.
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(
                    SELECT 1
                    FROM streams s
                    JOIN channels c ON c.id = s.channel_id
                    WHERE s.id = $1
                      AND s.status = 'live'
                      AND s.visibility = 'public'
                      AND c.is_mature = false
                )",
            )
            .bind(stream_id)
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

    match authorize(db, &input).await {
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
    use super::{constant_time_equal, parse_stream_path, valid_publish_key};
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
    fn media_publish_keys_are_strictly_validated() {
        assert!(valid_publish_key(&format!("lt_pub_{}", "a".repeat(64))));
        assert!(!valid_publish_key("lt_pub_short"));
        assert!(!valid_publish_key(&format!("lt_pub_{}", "z".repeat(64))));
    }
}
