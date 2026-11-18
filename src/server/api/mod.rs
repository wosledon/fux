//! REST/SSE 路由汇总（PRD 5.5）。
pub mod agents;
pub mod approvals;
pub mod assistant;
pub mod auth;
pub mod boards;
pub mod cli_entries;
pub mod mcp;
pub mod nodes;
pub mod notifications;
pub mod projects;
pub mod skills;
pub mod sse;
pub mod task_types;
pub mod tasks;
pub mod teams;
pub mod workspaces;

pub fn routes(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.configure(auth::routes)
        .configure(workspaces::routes)
        .configure(projects::routes)
        .configure(boards::routes)
        .configure(task_types::routes)
        .configure(tasks::routes)
        .configure(agents::routes)
        .configure(teams::routes)
        .configure(cli_entries::routes)
        .configure(skills::routes)
        .configure(mcp::routes)
        .configure(nodes::routes)
        .configure(approvals::routes)
        .configure(notifications::routes)
        .configure(assistant::routes)
        .configure(sse::routes);
}

/// 从 Header 或 ?token= 解析 JWT（SSE/EventSource 无法带 Header）。
pub fn claims_from_req(
    req: &actix_web::HttpRequest,
    secret: &str,
) -> Result<crate::server::auth::Claims, crate::server::error::ApiError> {
    use crate::server::error::ApiError;
    let header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(String::from);
    let query_token = req
        .query_string()
        .split('&')
        .find_map(|kv| kv.strip_prefix("token="))
        .map(|s| urldecode(s));
    let token = header.or(query_token).ok_or_else(|| ApiError::unauthorized("未登录"))?;
    jsonwebtoken::decode::<crate::server::auth::Claims>(
        &token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .map(|d| d.claims)
    .map_err(|_| ApiError::unauthorized("登录已过期"))
}

fn urldecode(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(b as char);
                    i += 3;
                    continue;
                }
                out.push('%');
                i += 1;
            }
            b'+' => {
                out.push(' ');
                i += 1;
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}