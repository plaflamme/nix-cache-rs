use std::convert::Infallible;

use axum::extract::FromRequestParts;
use http::Method as HttpMethod;

pub struct Method(pub HttpMethod);

impl<S> FromRequestParts<S> for Method
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(Method(parts.method.clone()))
    }
}
