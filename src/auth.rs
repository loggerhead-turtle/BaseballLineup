use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::{FromRequestParts, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rand::RngCore;
use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::models::AuthPayload;
use crate::AppState;

const COOKIE_NAME: &str = "session";
const SESSION_DAYS: i64 = 30;

/// Authenticated user, extracted from the session cookie.
pub struct AuthUser {
    pub id: i64,
    pub email: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = cookie_value(&parts.headers, COOKIE_NAME)
            .ok_or_else(|| AppError::unauthorized("not logged in"))?;

        let row = sqlx::query_as::<_, (i64, String)>(
            "SELECT users.id, users.email FROM sessions \
             JOIN users ON users.id = sessions.user_id \
             WHERE sessions.token = ? AND sessions.expires_at > datetime('now')",
        )
        .bind(&token)
        .fetch_optional(&state.pool)
        .await?;

        match row {
            Some((id, email)) => Ok(AuthUser { id, email }),
            None => Err(AppError::unauthorized("session expired")),
        }
    }
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix(&format!("{name}=")) {
            return Some(rest.to_string());
        }
    }
    None
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn set_cookie(token: &str) -> String {
    format!(
        "{COOKIE_NAME}={token}; HttpOnly; Path=/; SameSite=Lax; Max-Age={}",
        SESSION_DAYS * 24 * 60 * 60
    )
}

fn clear_cookie() -> String {
    format!("{COOKIE_NAME}=; HttpOnly; Path=/; SameSite=Lax; Max-Age=0")
}

fn hash_password(password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(hash.to_string())
}

fn verify_password(password: &str, stored: &str) -> bool {
    match PasswordHash::new(stored) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

async fn create_session(state: &AppState, user_id: i64) -> AppResult<String> {
    let token = new_token();
    sqlx::query(
        "INSERT INTO sessions (token, user_id, expires_at) \
         VALUES (?, ?, datetime('now', ?))",
    )
    .bind(&token)
    .bind(user_id)
    .bind(format!("+{SESSION_DAYS} days"))
    .execute(&state.pool)
    .await?;
    Ok(token)
}

pub async fn signup(
    State(state): State<AppState>,
    Json(payload): Json<AuthPayload>,
) -> AppResult<Response> {
    let email = payload.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::bad_request("a valid email is required"));
    }
    if payload.password.len() < 6 {
        return Err(AppError::bad_request(
            "password must be at least 6 characters",
        ));
    }

    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&state.pool)
        .await?;
    if exists.is_some() {
        return Err(AppError::conflict("an account with that email already exists"));
    }

    let hash = hash_password(&payload.password)?;
    let rec: (i64,) =
        sqlx::query_as("INSERT INTO users (email, password_hash) VALUES (?, ?) RETURNING id")
            .bind(&email)
            .bind(&hash)
            .fetch_one(&state.pool)
            .await?;
    let user_id = rec.0;

    let token = create_session(&state, user_id).await?;
    Ok((
        StatusCode::CREATED,
        [(SET_COOKIE, set_cookie(&token))],
        Json(json!({ "id": user_id, "email": email })),
    )
        .into_response())
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<AuthPayload>,
) -> AppResult<Response> {
    let email = payload.email.trim().to_lowercase();
    let user: Option<(i64, String)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?")
            .bind(&email)
            .fetch_optional(&state.pool)
            .await?;

    let (user_id, hash) = user.ok_or_else(|| AppError::unauthorized("invalid email or password"))?;
    if !verify_password(&payload.password, &hash) {
        return Err(AppError::unauthorized("invalid email or password"));
    }

    let token = create_session(&state, user_id).await?;
    Ok((
        StatusCode::OK,
        [(SET_COOKIE, set_cookie(&token))],
        Json(json!({ "id": user_id, "email": email })),
    )
        .into_response())
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Response> {
    if let Some(token) = cookie_value(&headers, COOKIE_NAME) {
        sqlx::query("DELETE FROM sessions WHERE token = ?")
            .bind(&token)
            .execute(&state.pool)
            .await?;
    }
    Ok((
        StatusCode::OK,
        [(SET_COOKIE, clear_cookie())],
        Json(json!({ "ok": true })),
    )
        .into_response())
}

pub async fn me(user: AuthUser) -> AppResult<Json<serde_json::Value>> {
    Ok(Json(json!({ "id": user.id, "email": user.email })))
}
