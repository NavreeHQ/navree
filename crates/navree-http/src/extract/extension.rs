//! [`Extension`]: a value the pipeline put in the request.

use std::ops::{Deref, DerefMut};

use http::request::Parts;

use super::rejection::ExtensionRejection;
use super::{FromRequestParts, OptionalFromRequestParts};

/// A `T` from the request extensions, cloned: what the pipeline and the
/// app's layers put there, such as the `AppContext`, the
/// `navree_contracts::RequestId` or a layer's own value. A missing `T` is a
/// 500 (the app lacks the layer that adds it); `Option<Extension<T>>` is
/// `None` instead.
///
/// ```
/// use http::Method;
/// use navree_http::{Extension, RouteDef};
/// use navree_kernel::AppContext;
///
/// async fn handler(Extension(ctx): Extension<AppContext>) -> &'static str {
///     let _ = ctx;
///     "ok"
/// }
///
/// let route = RouteDef::on(Method::GET, "/", handler).public();
/// # let _ = route;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extension<T>(pub T);

impl<T> Deref for Extension<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Extension<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: Clone + Send + Sync + 'static> FromRequestParts for Extension<T> {
    type Rejection = ExtensionRejection;

    async fn from_request_parts(parts: &mut Parts) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<T>()
            .cloned()
            .map(Extension)
            .ok_or(ExtensionRejection(std::any::type_name::<T>()))
    }
}

impl<T: Clone + Send + Sync + 'static> OptionalFromRequestParts for Extension<T> {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts) -> Result<Option<Self>, Self::Rejection> {
        Ok(parts.extensions.get::<T>().cloned().map(Extension))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn values_come_from_the_extensions() {
        let (mut parts, ()) = http::Request::new(()).into_parts();
        let missing = <Extension<u8> as FromRequestParts>::from_request_parts(&mut parts).await;
        assert_eq!(
            missing.unwrap_err().body_text(),
            "Missing request extension: Extension of type `u8` was not found"
        );
        let none = <Extension<u8> as OptionalFromRequestParts>::from_request_parts(&mut parts)
            .await
            .unwrap();
        assert!(none.is_none());
        parts.extensions.insert(7u8);
        let Extension(value) = <Extension<u8> as FromRequestParts>::from_request_parts(&mut parts)
            .await
            .unwrap();
        assert_eq!(value, 7);
    }
}
