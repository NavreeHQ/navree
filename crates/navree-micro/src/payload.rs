//! [`Payload<T>`]: the decoded payload of a message.

use std::ops::{Deref, DerefMut};

use axum::extract::{FromRequest, Request};
use bytes::Bytes;
use http::StatusCode;
use http::header::CONTENT_TYPE;
use navree_http::AppError;
use navree_validation::ValidInput;
use navree_validation::garde::Validate;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::Codec;

/// The message's payload, decoded into `T` with the codec of its
/// `content-type` (`json` or `msgpack`).
///
/// A payload that does not decode is a 400 reply with serde's message.
/// `Valid<Payload<T>>` and `ValidAsync<Payload<T>>` (navree-validation)
/// also run the garde rules and the database rules of `T`, with the same
/// field errors as `Valid<Json<T>>` on a route.
///
/// ```ignore
/// #[message_pattern("deals.move")]
/// #[can(Move, Deal, from = "id")]
/// async fn move_deal(&self, uow: Uow, Valid(Payload(input)): Valid<Payload<MoveDeal>>) -> Result<DealView> {
///     // ...
/// }
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Payload<T>(pub T);

impl<T> Payload<T> {
    /// The decoded value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Payload<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Payload<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

/// The codec of the request's `content-type`; a 415 for anything else.
fn codec_of(req: &Request) -> Result<Codec, AppError> {
    let content_type = req
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json");
    Codec::from_content_type(content_type).ok_or_else(|| {
        AppError::from_status(StatusCode::UNSUPPORTED_MEDIA_TYPE).with_detail(format!(
            "Expected a payload of `application/json` or `application/msgpack`, not `{content_type}`"
        ))
    })
}

async fn bytes_of(req: Request) -> Result<Bytes, AppError> {
    Bytes::from_request(req, &()).await.map_err(|rejection| {
        AppError::from_status(rejection.status()).with_detail(rejection.body_text())
    })
}

impl<T, S> FromRequest<S> for Payload<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let codec = codec_of(&req)?;
        let bytes = bytes_of(req).await?;
        codec
            .decode(&bytes)
            .map(Payload)
            .map_err(|error| AppError::bad_request(format!("The payload does not decode: {error}")))
    }
}

/// `Valid<Payload<T>>` and `ValidAsync<Payload<T>>`.
impl<T> ValidInput for Payload<T>
where
    T: DeserializeOwned + Validate<Context = ()> + Send,
{
    type Target = T;

    async fn read(req: Request) -> Result<Option<Value>, AppError> {
        let codec = codec_of(&req)?;
        let bytes = bytes_of(req).await?;
        match codec {
            Codec::Json => Ok(serde_json::from_slice(&bytes).ok()),
            Codec::Msgpack => rmp_serde::from_slice::<Value>(&bytes)
                .map(Some)
                .map_err(|_| AppError::bad_request("The payload is not valid MessagePack")),
        }
    }

    fn wrap(value: T) -> Self {
        Payload(value)
    }
}
