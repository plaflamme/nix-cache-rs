use std::time::Duration;

use aws_sigv4::{
    SigningOutput,
    http_request::{
        SignableRequest, SignatureLocation, SigningInstructions, SigningParams, SigningSettings,
    },
};

use crate::NixCacheApp;

pub fn sign_request<'a>(
    app: &NixCacheApp,
    request: SignableRequest<'a>,
) -> Result<SigningOutput<SigningInstructions>, crate::Error> {
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

    let result = aws_sigv4::http_request::sign(request, &signing_params)?;
    Ok(result)
}
