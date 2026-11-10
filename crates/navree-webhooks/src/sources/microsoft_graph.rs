//! [`MicrosoftGraph`]: change and lifecycle notifications, checked by their
//! `clientState`; the `validationToken` handshake.

use http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::options::SourceOptions;
use crate::request::{Handshake, IncomingRequest, Verdict};
use crate::signing;
use crate::verify::{
    Tolerance, Verified, VerifyError, WebhookSource, WebhookVerifier, require_secret,
};

/// Microsoft Graph change notifications. Graph signs nothing: each
/// notification carries the `clientState` given when the subscription was
/// created, and every one in the batch must equal the source's secret
/// (compared in constant time). Lifecycle notifications
/// (`reauthorizationRequired`, `subscriptionRemoved`, `missed`) are checked
/// the same way. When a subscription is created, Graph calls the URL with a
/// `validationToken` query parameter and expects it back as `text/plain`
/// within ten seconds: that request is answered without running the
/// handler. The event type is the first notification's `lifecycleEvent`,
/// else its `changeType`. Source name `microsoft_graph`.
///
/// Read the body as [`GraphNotifications`]. Graph expects a `202 Accepted`
/// within three seconds: queue the work.
#[derive(Clone, Copy, Debug, Default)]
pub struct MicrosoftGraph;

impl MicrosoftGraph {
    /// The query parameter of the validation handshake.
    pub const VALIDATION_TOKEN: &'static str = "validationToken";

    /// Verifies a batch of notifications: every `clientState` must equal
    /// `client_state`.
    pub fn verify(client_state: &str, body: &[u8]) -> Result<Verified, VerifyError> {
        require_secret(client_state)?;
        let batch: GraphNotifications = serde_json::from_slice(body)
            .map_err(|_| VerifyError::MalformedBody("Graph notification batch".to_owned()))?;
        let Some(first) = batch.value.first() else {
            return Err(VerifyError::MalformedBody("notification".to_owned()));
        };
        let mut matched = true;
        for notification in &batch.value {
            // Check them all, so the time does not depend on which differs.
            let sent = notification.client_state.as_deref().unwrap_or_default();
            matched &= signing::same(sent.as_bytes(), client_state.as_bytes());
        }
        if !matched {
            return Err(VerifyError::SignatureMismatch);
        }
        Ok(Verified {
            id: None,
            timestamp: None,
            event_type: first
                .lifecycle_event
                .clone()
                .or_else(|| first.change_type.clone()),
        })
    }
}

impl WebhookVerifier for MicrosoftGraph {
    fn verify_request(
        source: &SourceOptions,
        _headers: &HeaderMap,
        body: &[u8],
        _tolerance: Option<Tolerance>,
    ) -> Result<Verified, VerifyError> {
        Self::verify(source.secret.expose(), body)
    }

    async fn verify_incoming(request: &IncomingRequest<'_>) -> Result<Verdict, VerifyError> {
        if let Some(token) = request.query(Self::VALIDATION_TOKEN) {
            return Ok(Verdict::Handshake(Handshake::text(token)));
        }
        Self::verify(request.source().secret.expose(), request.body()).map(Verdict::Verified)
    }
}

impl WebhookSource for MicrosoftGraph {
    const NAME: &'static str = "microsoft_graph";
    type Verifier = MicrosoftGraph;
}

/// The body of a Microsoft Graph notification request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNotifications {
    /// The notifications.
    pub value: Vec<GraphNotification>,
    /// The tokens of notifications with resource data, when the
    /// subscription asked for them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validation_tokens: Vec<String>,
}

/// One change or lifecycle notification.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNotification {
    /// The subscription that produced it.
    pub subscription_id: String,
    /// When the subscription expires (ISO 8601).
    #[serde(default)]
    pub subscription_expiration_date_time: Option<String>,
    /// The subscription's `clientState` (already checked).
    #[serde(default)]
    pub client_state: Option<String>,
    /// `created`, `updated` or `deleted`, for a change notification.
    #[serde(default)]
    pub change_type: Option<String>,
    /// The changed resource (`users/{id}/messages/{id}`).
    #[serde(default)]
    pub resource: Option<String>,
    /// What Graph says about the resource (`@odata.type`, `id`).
    #[serde(default)]
    pub resource_data: Option<Value>,
    /// The tenant.
    #[serde(default)]
    pub tenant_id: Option<String>,
    /// `reauthorizationRequired`, `subscriptionRemoved` or `missed`, for a
    /// lifecycle notification.
    #[serde(default)]
    pub lifecycle_event: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::sources::block_on;

    const STATE: &str = "secretClientValue";

    fn batch(states: &[&str]) -> Vec<u8> {
        let value: Vec<Value> = states
            .iter()
            .map(|state| {
                json!({
                    "subscriptionId": "7f105c7d-2dc5-4530-97cd-4e7ae6534c07",
                    "subscriptionExpirationDateTime": "2026-10-29T08:00:00.000Z",
                    "clientState": state,
                    "changeType": "created",
                    "resource": "Users/7a6e/Messages/AAMkAD",
                    "tenantId": "84bd8158-6d4d-4958-8b9f-9d6445542f95",
                    "resourceData": {
                        "@odata.type": "#Microsoft.Graph.Message",
                        "id": "AAMkAD"
                    }
                })
            })
            .collect();
        serde_json::to_vec(&json!({ "value": value })).unwrap()
    }

    #[test]
    fn every_client_state_must_match() {
        let verified = MicrosoftGraph::verify(STATE, &batch(&[STATE, STATE])).unwrap();
        assert_eq!(verified.event_type.as_deref(), Some("created"));
        assert_eq!(verified.id, None);
        assert_eq!(
            MicrosoftGraph::verify(STATE, &batch(&[STATE, "other"])),
            Err(VerifyError::SignatureMismatch)
        );
        let missing = br#"{"value":[{"subscriptionId":"s","changeType":"updated"}]}"#;
        assert_eq!(
            MicrosoftGraph::verify(STATE, missing),
            Err(VerifyError::SignatureMismatch)
        );
        for malformed in [&br#"{"value":[]}"#[..], b"[]", b"validationToken"] {
            assert!(matches!(
                MicrosoftGraph::verify(STATE, malformed),
                Err(VerifyError::MalformedBody(_))
            ));
        }
    }

    #[test]
    fn lifecycle_notifications_carry_their_event() {
        let body = serde_json::to_vec(&json!({
            "value": [{
                "subscriptionId": "7f105c7d",
                "subscriptionExpirationDateTime": "2026-10-29T08:00:00.000Z",
                "tenantId": "84bd8158",
                "clientState": STATE,
                "lifecycleEvent": "reauthorizationRequired"
            }]
        }))
        .unwrap();
        let verified = MicrosoftGraph::verify(STATE, &body).unwrap();
        assert_eq!(
            verified.event_type.as_deref(),
            Some("reauthorizationRequired")
        );
        let parsed: GraphNotifications = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            parsed.value[0].lifecycle_event.as_deref(),
            Some("reauthorizationRequired")
        );
    }

    #[test]
    fn the_validation_token_is_echoed_without_the_handler() {
        let source = SourceOptions::new(STATE);
        let headers = HeaderMap::new();
        let request = IncomingRequest::new(&source, &headers, b"").with_url(
            "https://api.example.com/api/hooks/graph?validationToken=Validation%3A%20Testing%20client%20application%20reachability",
        );
        assert_eq!(
            block_on(MicrosoftGraph::verify_incoming(&request)),
            Ok(Verdict::Handshake(Handshake::text(
                "Validation: Testing client application reachability"
            )))
        );
        let body = batch(&[STATE]);
        let notification = IncomingRequest::new(&source, &headers, &body)
            .with_url("https://api.example.com/api/hooks/graph");
        assert!(matches!(
            block_on(MicrosoftGraph::verify_incoming(&notification)),
            Ok(Verdict::Verified(_))
        ));
    }
}
