use std::time::Duration;

use aws_sigv4::http_request::{SignableRequest, SignatureLocation, SigningParams, SigningSettings};

use crate::NixCacheApp;

/// Uses the provided request to generate signing query parameters and adds the resulting parameters to the provided [`request_url`].
pub fn sign_request(
    app: &NixCacheApp,
    request_url: &mut url::Url,
    method: http::Method,
    headers: &[(&str, &str)],
) -> Result<(), crate::Error> {
    let identity = app.r2_credentials()?.into();
    let mut settings = SigningSettings::default();
    settings.signature_location = SignatureLocation::QueryParams;
    settings.expires_in = Some(Duration::from_hours(1));
    let signing_params = SigningParams::V4(
        aws_sigv4::sign::v4::SigningParams::builder()
            .identity(&identity)
            .region("auto")
            .name("s3")
            .time(crate::time::now())
            .settings(settings)
            .build()?,
    );

    let method = method.to_string();
    let request = SignableRequest::new(
        &method,
        request_url.to_string(),
        headers.iter().cloned(),
        aws_sigv4::http_request::SignableBody::UnsignedPayload,
    )?;

    let result = aws_sigv4::http_request::sign(request, &signing_params)?;
    result
        .output()
        .params()
        .iter()
        .fold(&mut request_url.query_pairs_mut(), |qp, (key, value)| {
            qp.append_pair(key, value)
        })
        .finish();
    Ok(())
}
