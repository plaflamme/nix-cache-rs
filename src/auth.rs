use axum::{
    extract::{Request, State},
    middleware::Next,
};
use http::StatusCode;

use crate::NixCacheApp;
use base64::{Engine, engine::general_purpose::STANDARD};

fn decode_basic_auth(basic: &str) -> Option<String> {
    let token = STANDARD.decode(basic).ok()?;
    let token = String::from_utf8(token).ok()?;
    let (left, right) = token.split_once(':')?;
    let token = if left.is_empty() { right } else { left };
    Some(token.to_string())
}

fn decode_auth_token(kind: &str, value: &str) -> Option<String> {
    match kind.to_lowercase().as_str() {
        "bearer" => Some(value.to_string()),
        "basic" => decode_basic_auth(value),
        _ => None,
    }
}

pub async fn authenticate(
    State(app): State<NixCacheApp>,
    request: Request,
    next: Next,
) -> axum::response::Response {
    if let Some(authorization) = request.headers().get(http::header::AUTHORIZATION)
        && let Ok(authorization) = authorization.to_str()
        && let Some((kind, param)) = authorization.split_once(' ')
        && let Some(token) = decode_auth_token(kind, param)
    {
        if token == app.auth_token {
            next.run(request).await
        } else {
            axum::response::Response::builder()
                .status(StatusCode::FORBIDDEN)
                .body(axum::body::Body::from("invalid authorization header"))
                .expect("statically known to be valid")
        }
    } else {
        axum::response::Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(axum::body::Body::empty())
            .expect("statically known to be valid")
    }
}
