use crate::state::AppState;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
fn pool(state: &AppState) -> Result<&PgPool, ApiError> {
    state.db.as_ref().ok_or(ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "database_unavailable",
    ))
}
fn secret(state: &AppState) -> Result<&str, ApiError> {
    state
        .jwt_secret
        .as_deref()
        .map(String::as_str)
        .ok_or(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_not_configured",
        ))
}
#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: String,
    exp: usize,
    iat: usize,
}
#[derive(Serialize)]
pub struct UserView {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}
#[derive(FromRow)]
struct UserRow {
    id: Uuid,
    email: String,
    display_name: String,
    avatar_url: Option<String>,
    password_hash: String,
    created_at: chrono::DateTime<Utc>,
}
impl From<UserRow> for UserView {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            email: row.email,
            display_name: row.display_name,
            avatar_url: row.avatar_url,
            created_at: row.created_at,
        }
    }
}
#[derive(Deserialize)]
pub struct RegisterRequest {
    email: String,
    password: String,
    display_name: String,
}
#[derive(Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}
#[derive(Deserialize)]
pub struct RefreshRequest {
    refresh_token: String,
}
#[derive(Serialize)]
pub struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: u64,
    refresh_token: String,
    user: UserView,
}
fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "password_hash_failed"))
}
fn verify_password(password: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).ok().is_some_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    })
}
fn create_access_token(user_id: Uuid, secret: &str) -> Result<String, ApiError> {
    let now = Utc::now();
    let claims = Claims {
        sub: user_id.to_string(),
        iss: "live-tron-api".to_owned(),
        aud: "live-tron-web".to_owned(),
        iat: now.timestamp() as usize,
        exp: (now + Duration::minutes(15)).timestamp() as usize,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "token_issue_failed"))
}
fn create_refresh_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn is_valid_refresh_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_valid_email(email: &str) -> bool {
    if email.len() > 254 || !email.is_ascii() {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    if local.is_empty()
        || local.len() > 64
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || domain.is_empty()
        || domain.starts_with('.')
        || domain.ends_with('.')
    {
        return false;
    }
    if !local.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'%' | b'+' | b'-')
    }) {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
        && labels.last().is_some_and(|label| label.len() >= 2)
}

async fn issue_session(
    db: &PgPool,
    user: UserRow,
    secret: &str,
) -> Result<Json<TokenResponse>, ApiError> {
    let refresh_token = create_refresh_token();
    sqlx::query(
        "INSERT INTO refresh_sessions (user_id, token_hash, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(user.id)
    .bind(token_hash(&refresh_token))
    .bind(Utc::now() + Duration::days(30))
    .execute(db)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "session_create_failed"))?;
    let access_token = create_access_token(user.id, secret)?;
    Ok(Json(TokenResponse {
        access_token,
        token_type: "Bearer",
        expires_in: 900,
        refresh_token,
        user: user.into(),
    }))
}

pub async fn register(
    State(state): State<AppState>,
    Json(input): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<TokenResponse>), ApiError> {
    let db = pool(&state)?;
    let jwt = secret(&state)?;
    let email = input.email.trim().to_lowercase();
    let display_name = input.display_name.trim();
    if !is_valid_email(&email) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_email"));
    }
    if !(2..=80).contains(&display_name.chars().count()) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_display_name"));
    }
    if !(12..=128).contains(&input.password.len()) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "password_length_invalid"));
    }
    let password_hash = hash_password(&input.password)?;
    let user = sqlx::query_as::<_, UserRow>("INSERT INTO users (email, password_hash, display_name) VALUES ($1, $2, $3) RETURNING id, email, display_name, avatar_url, password_hash, created_at")
        .bind(email).bind(password_hash).bind(display_name).fetch_one(db).await.map_err(|error| {
            if error.as_database_error().is_some_and(|e| e.code().as_deref() == Some("23505")) {
                ApiError(StatusCode::CONFLICT, "email_already_registered")
            } else { tracing::error!(error = %error, "user registration failed"); ApiError(StatusCode::INTERNAL_SERVER_ERROR, "registration_failed") }
        })?;
    Ok((StatusCode::CREATED, issue_session(db, user, jwt).await?))
}

pub async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    let db = pool(&state)?;
    let jwt = secret(&state)?;
    let email = input.email.trim().to_lowercase();
    if !is_valid_email(&email) {
        let _ = hash_password(&input.password)?;
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials"));
    }
    let user = sqlx::query_as::<_, UserRow>("SELECT id, email, display_name, avatar_url, password_hash, created_at FROM users WHERE lower(email) = $1")
        .bind(email).fetch_optional(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "login_failed"))?;
    let Some(user) = user else {
        // Spend one Argon2 operation for unknown accounts to reduce email enumeration by timing.
        let _ = hash_password(&input.password)?;
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials"));
    };
    if !verify_password(&input.password, &user.password_hash) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_credentials"));
    }
    issue_session(db, user, jwt).await
}

pub async fn refresh(
    State(state): State<AppState>,
    Json(input): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    let db = pool(&state)?;
    let jwt = secret(&state)?;
    if !is_valid_refresh_token(&input.refresh_token) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "invalid_refresh_token"));
    }
    let old_hash = token_hash(&input.refresh_token);
    let mut tx = db
        .begin()
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "session_refresh_failed"))?;
    let user_id = sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM refresh_sessions WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now() FOR UPDATE")
        .bind(old_hash.clone()).fetch_optional(&mut *tx).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "session_refresh_failed"))?
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "invalid_refresh_token"))?;
    sqlx::query("UPDATE refresh_sessions SET revoked_at = now() WHERE token_hash = $1")
        .bind(old_hash)
        .execute(&mut *tx)
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "session_refresh_failed"))?;
    let user = sqlx::query_as::<_, UserRow>("SELECT id, email, display_name, avatar_url, password_hash, created_at FROM users WHERE id = $1")
        .bind(user_id).fetch_one(&mut *tx).await
        .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "user_not_found"))?;
    let refresh_token = create_refresh_token();
    sqlx::query(
        "INSERT INTO refresh_sessions (user_id, token_hash, expires_at) VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(token_hash(&refresh_token))
    .bind(Utc::now() + Duration::days(30))
    .execute(&mut *tx)
    .await
    .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "session_refresh_failed"))?;
    tx.commit()
        .await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "session_refresh_failed"))?;
    Ok(Json(TokenResponse {
        access_token: create_access_token(user.id, jwt)?,
        token_type: "Bearer",
        expires_in: 900,
        refresh_token,
        user: user.into(),
    }))
}

pub async fn logout(
    State(state): State<AppState>,
    Json(input): Json<RefreshRequest>,
) -> Result<StatusCode, ApiError> {
    let db = pool(&state)?;
    if !is_valid_refresh_token(&input.refresh_token) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_refresh_token"));
    }
    sqlx::query("UPDATE refresh_sessions SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL")
        .bind(token_hash(&input.refresh_token)).execute(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "logout_failed"))?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn authenticated_user(headers: &HeaderMap, state: &AppState) -> Result<Uuid, ApiError> {
    let jwt = secret(state)?;
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "missing_bearer_token"))?;
    let mut validation = Validation::default();
    validation.set_issuer(&["live-tron-api"]);
    validation.set_audience(&["live-tron-web"]);
    let claims = decode::<Claims>(
        value,
        &DecodingKey::from_secret(jwt.as_bytes()),
        &validation,
    )
    .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "invalid_access_token"))?;
    Uuid::parse_str(&claims.claims.sub)
        .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "invalid_access_token"))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<UserView>, ApiError> {
    let user_id = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    let user = sqlx::query_as::<_, UserRow>("SELECT id, email, display_name, avatar_url, password_hash, created_at FROM users WHERE id = $1")
        .bind(user_id).fetch_optional(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "profile_read_failed"))?
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "user_not_found"))?;
    Ok(Json(user.into()))
}

#[derive(Deserialize)]
pub struct UpdateProfileRequest {
    display_name: Option<String>,
}
pub async fn update_me(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateProfileRequest>,
) -> Result<Json<UserView>, ApiError> {
    let user_id = authenticated_user(&headers, &state)?;
    let db = pool(&state)?;
    let display_name = input.display_name.map(|v| v.trim().to_owned());
    if display_name
        .as_ref()
        .is_some_and(|v| !(2..=80).contains(&v.chars().count()))
    {
        return Err(ApiError(StatusCode::BAD_REQUEST, "invalid_display_name"));
    }
    let user = sqlx::query_as::<_, UserRow>("UPDATE users SET display_name = COALESCE($2, display_name), updated_at = now() WHERE id = $1 RETURNING id, email, display_name, avatar_url, password_hash, created_at")
        .bind(user_id).bind(display_name).fetch_optional(db).await
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "profile_update_failed"))?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "user_not_found"))?;
    Ok(Json(user.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_hashes_are_salted_and_verifiable() {
        let first = hash_password("a-long-test-password").unwrap();
        let second = hash_password("a-long-test-password").unwrap();
        assert_ne!(first, second);
        assert!(verify_password("a-long-test-password", &first));
        assert!(!verify_password("wrong-password", &first));
    }
    #[test]
    fn refresh_tokens_are_random_and_stored_as_hashes() {
        let first = create_refresh_token();
        let second = create_refresh_token();
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
        assert_ne!(token_hash(&first), first);
        assert!(is_valid_refresh_token(&first));
        assert!(!is_valid_refresh_token("not-a-token"));
        assert!(!is_valid_refresh_token(&"z".repeat(64)));
    }

    #[test]
    fn email_validation_rejects_malformed_addresses() {
        assert!(is_valid_email("creator+live@example.test"));
        assert!(is_valid_email("a.b_1@example.co.ke"));
        assert!(!is_valid_email("missing-at.example.test"));
        assert!(!is_valid_email("@example.test"));
        assert!(!is_valid_email("name@"));
        assert!(!is_valid_email("name@example"));
        assert!(!is_valid_email("name..dot@example.test"));
        assert!(!is_valid_email("name@-example.test"));
        assert!(!is_valid_email("name@example-.test"));
        assert!(!is_valid_email("name with space@example.test"));
    }

    #[test]
    fn access_tokens_are_bound_to_live_tron_issuer_and_audience() {
        let secret = "integration-test-secret-with-at-least-32-bytes";
        let token = create_access_token(Uuid::new_v4(), secret).unwrap();
        let mut validation = Validation::default();
        validation.set_issuer(&["live-tron-api"]);
        validation.set_audience(&["live-tron-web"]);
        assert!(decode::<Claims>(
            &token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation
        )
        .is_ok());

        validation.set_audience(&["untrusted-client"]);
        assert!(decode::<Claims>(
            &token,
            &DecodingKey::from_secret(secret.as_bytes()),
            &validation
        )
        .is_err());
    }
}
