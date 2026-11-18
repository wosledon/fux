use crate::server::auth::{hash_password, issue_token, verify_password, AuthUser};
use crate::server::error::{ApiError, ApiResult};
use crate::server::models::User;
use crate::server::SharedState;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterReq {
    username: String,
    password: String,
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

async fn register(state: SharedState, body: web::Json<RegisterReq>) -> ApiResult<HttpResponse> {
    let username = body.username.trim().to_lowercase();
    if username.len() < 3 || username.len() > 32 {
        return Err(ApiError::bad_request("用户名需 3-32 个字符"));
    }
    if body.password.len() < 8 {
        return Err(ApiError::bad_request("密码至少 8 位"));
    }
    let exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM users WHERE username = ?").bind(&username).fetch_optional(&state.db).await?;
    if exists.is_some() {
        return Err(ApiError::conflict("用户名已存在"));
    }
    let id = crate::new_id();
    let hash = hash_password(&body.password)?;
    let display = body.display_name.clone().unwrap_or_else(|| username.clone());
    sqlx::query("INSERT INTO users (id, username, password_hash, display_name) VALUES (?,?,?,?)")
        .bind(&id)
        .bind(&username)
        .bind(&hash)
        .bind(&display)
        .execute(&state.db)
        .await?;
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?").bind(&id).fetch_one(&state.db).await?;
    let token = issue_token(&state.jwt_secret, &id, &username)?;
    Ok(HttpResponse::Ok().json(json!({ "token": token, "user": user })))
}

async fn login(state: SharedState, body: web::Json<LoginReq>) -> ApiResult<HttpResponse> {
    let username = body.username.trim().to_lowercase();
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
        .bind(&username)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::unauthorized("用户名或密码错误"))?;
    if !verify_password(&body.password, &user.password_hash) {
        return Err(ApiError::unauthorized("用户名或密码错误"));
    }
    let token = issue_token(&state.jwt_secret, &user.id, &user.username)?;
    Ok(HttpResponse::Ok().json(json!({ "token": token, "user": user })))
}

async fn me(state: SharedState, user: AuthUser) -> ApiResult<HttpResponse> {
    let u = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?").bind(&user.id).fetch_one(&state.db).await?;
    Ok(HttpResponse::Ok().json(u))
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/auth/register", web::post().to(register))
        .route("/auth/login", web::post().to(login))
        .route("/me", web::get().to(me));
}