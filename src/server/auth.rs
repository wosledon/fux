use crate::server::error::{ApiError, ApiResult};
use actix_web::{dev::Payload, FromRequest, HttpRequest};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::future::{ready, Ready};

pub fn hash_password(pw: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(pw.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash 失败: {e}"))?
        .to_string())
}

pub fn verify_password(pw: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default().verify_password(pw.as_bytes(), &parsed).is_ok(),
        Err(_) => false,
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub exp: usize,
}

pub fn issue_token(secret: &str, user_id: &str, username: &str) -> ApiResult<String> {
    let exp = (chrono::Utc::now() + chrono::Duration::hours(24 * 7)).timestamp() as usize;
    encode(
        &Header::default(),
        &Claims { sub: user_id.to_string(), username: username.to_string(), exp },
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| ApiError::new(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// 已认证用户（Bearer JWT）。
pub struct AuthUser {
    pub id: String,
    pub username: String,
}

impl FromRequest for AuthUser {
    type Error = ApiError;
    type Future = Ready<ApiResult<Self>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        let secret = req
            .app_data::<actix_web::web::Data<crate::server::AppState>>()
            .map(|s| s.jwt_secret.clone())
            .unwrap_or_default();
        let token = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string());
        let result = match token {
            Some(t) => decode::<Claims>(
                &t,
                &DecodingKey::from_secret(secret.as_bytes()),
                &Validation::default(),
            )
            .map(|d| AuthUser { id: d.claims.sub, username: d.claims.username })
            .map_err(|_| ApiError::unauthorized("登录已过期，请重新登录")),
            None => Err(ApiError::unauthorized("未登录")),
        };
        ready(result)
    }
}

pub fn role_rank(role: &str) -> u8 {
    match role {
        "owner" => 3,
        "admin" => 2,
        "member" => 1,
        _ => 0,
    }
}

pub async fn role_of(db: &SqlitePool, workspace_id: &str, user_id: &str) -> Option<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT role FROM workspace_members WHERE workspace_id = ? AND user_id = ?",
    )
    .bind(workspace_id)
    .bind(user_id)
    .fetch_optional(db)
    .await
    .ok()
    .flatten()
}

/// 要求至少 min 角色（member/admin/owner）。
pub async fn require(
    db: &SqlitePool,
    workspace_id: &str,
    user_id: &str,
    min: &str,
) -> ApiResult<String> {
    match role_of(db, workspace_id, user_id).await {
        Some(r) if role_rank(&r) >= role_rank(min) => Ok(r),
        Some(_) => Err(ApiError::forbidden("权限不足")),
        None => Err(ApiError::forbidden("不是该工作区成员")),
    }
}