//! [`Form`]: an `application/x-www-form-urlencoded` body.

use std::ops::{Deref, DerefMut};

use http::Method;
use serde::de::DeserializeOwned;

use super::media::MediaType;
use super::rejection::FormRejection;
use super::{FromRequest, Request, urlencoded};
use crate::body::{BodyLimit, read_body};

/// A form, deserialized into `T` (see [`urlencoded`](super::urlencoded)):
/// for `GET` and `HEAD` the query string, for other methods an
/// `application/x-www-form-urlencoded` body read within the route's
/// [`BodyLimit`].
///
/// Rejections: 415 for a body of another content type, 413 for one past
/// the limit, 400 for one that does not deserialize.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Form<T>(pub T);

impl<T> Deref for Form<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Form<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: DeserializeOwned + Send> FromRequest for Form<T> {
    type Rejection = FormRejection;

    async fn from_request(request: Request) -> Result<Self, Self::Rejection> {
        if request.method() == Method::GET || request.method() == Method::HEAD {
            let query = request.uri().query().unwrap_or_default();
            return deserialize(query.as_bytes()).map_err(FormRejection::Query);
        }
        if !MediaType::of(request.headers())
            .is_some_and(|media| media.is("application", "x-www-form-urlencoded"))
        {
            return Err(FormRejection::ContentType);
        }
        let limit = BodyLimit::of(request.extensions());
        let bytes = read_body(request.into_body(), limit)
            .await
            .map_err(FormRejection::Body)?;
        deserialize(&bytes).map_err(FormRejection::Deserialize)
    }
}

fn deserialize<T: DeserializeOwned>(input: &[u8]) -> Result<Form<T>, String> {
    serde_path_to_error::deserialize(urlencoded::Deserializer::new(input))
        .map(Form)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;
    use crate::body::Body;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Login {
        email: String,
        remember: Option<bool>,
    }

    async fn form(request: Request) -> Result<Login, (u16, String)> {
        Form::<Login>::from_request(request)
            .await
            .map(|Form(login)| login)
            .map_err(|rejection| (rejection.status().as_u16(), rejection.to_string()))
    }

    fn post(content_type: Option<&str>, body: &'static str) -> Request {
        let mut builder = http::Request::post("/login");
        if let Some(content_type) = content_type {
            builder = builder.header("content-type", content_type);
        }
        builder.body(Body::from(body)).unwrap()
    }

    #[tokio::test]
    async fn bodies_need_the_form_content_type() {
        let login = form(post(
            Some("application/x-www-form-urlencoded"),
            "email=anu%40example.com&remember=true",
        ))
        .await
        .unwrap();
        assert_eq!(
            login,
            Login {
                email: "anu@example.com".into(),
                remember: Some(true)
            }
        );
        assert!(
            form(post(
                Some("Application/X-WWW-Form-Urlencoded; charset=utf-8"),
                "email=a"
            ))
            .await
            .is_ok()
        );
        assert_eq!(
            form(post(Some("text/plain"), "email=a")).await.unwrap_err(),
            (
                415,
                "Form requests must have `Content-Type: application/x-www-form-urlencoded`".into()
            )
        );
        assert_eq!(form(post(None, "email=a")).await.unwrap_err().0, 415);
        assert_eq!(
            form(post(
                Some("application/x-www-form-urlencoded"),
                "remember=true"
            ))
            .await
            .unwrap_err(),
            (
                400,
                "Failed to deserialize form body: missing field `email`".into()
            )
        );
    }

    #[tokio::test]
    async fn get_and_head_read_the_query_string() {
        let request = http::Request::get("/login?email=a%2Bb%40x&remember=false")
            .body(Body::empty())
            .unwrap();
        assert_eq!(form(request).await.unwrap().email, "a+b@x");
        let request = http::Request::head("/login").body(Body::empty()).unwrap();
        assert_eq!(
            form(request).await.unwrap_err(),
            (
                400,
                "Failed to deserialize form: missing field `email`".into()
            )
        );
    }

    #[tokio::test]
    async fn bodies_stop_at_the_limit() {
        let mut request = post(Some("application/x-www-form-urlencoded"), "email=abcdefgh");
        request.extensions_mut().insert(BodyLimit(8));
        assert_eq!(form(request).await.unwrap_err().0, 413);
    }
}
