//! [`SnsSms`]: Amazon SNS `Publish` to a phone number.

use std::fmt;

use chrono::Utc;
use navree_contracts::BoxFuture;
use navree_crypto::sigv4::{Credentials, Signer};
use navree_http_client::HttpClient;
use url::Url;

use super::{ChannelError, SentMessage, SmsDriver, client_for, endpoint, unreachable};
use crate::{SmsMessage, SmsOptions, SnsOptions};

/// The provider name of the SNS driver.
const PROVIDER: &str = "sns";

/// The form content type SNS's query API takes.
const FORM: &str = "application/x-www-form-urlencoded; charset=utf-8";

/// Sends text messages with Amazon SNS's `Publish` action (SigV4 signed),
/// as `notify.sns.sms_type` (`Transactional` by default). A sender from
/// [`SmsMessage::from`](crate::SmsMessage::from) or `notify.sms.from` is
/// the origination number when it starts with `+`, else the sender id. The
/// message id is SNS's `MessageId`. SNS reports delivery statuses to
/// CloudWatch Logs, not to a callback.
pub struct SnsSms {
    signer: Signer,
    sms_type: String,
    base: Url,
    client: HttpClient,
    from: Option<String>,
}

impl SnsSms {
    /// A driver for the credentials in `sns` and the defaults in `sms`.
    pub fn new(sns: &SnsOptions, sms: &SmsOptions) -> Result<Self, String> {
        if sns.region.is_empty()
            || sns.access_key_id.is_empty()
            || sns.secret_access_key.expose().is_empty()
        {
            return Err("the sns driver needs notify.sns.region, notify.sns.access_key_id and notify.sns.secret_access_key".into());
        }
        let default = format!("https://sns.{}.amazonaws.com", sns.region);
        let base = endpoint(sns.endpoint.as_deref(), &default, "notify.sns.endpoint")?;
        let client = client_for(sns.endpoint.as_ref().map(|_| &base))?;
        let credentials = Credentials::new(
            sns.access_key_id.clone(),
            sns.secret_access_key.expose().clone(),
        )
        .with_session_token(sns.session_token.as_ref().map(|t| t.expose().clone()));
        Ok(Self {
            signer: Signer::new(credentials, sns.region.clone(), "sns"),
            sms_type: sns.sms_type.clone(),
            base,
            client,
            from: sms.from.clone(),
        })
    }

    /// This driver, sending with `client`.
    #[must_use]
    pub fn with_client(mut self, client: HttpClient) -> Self {
        self.client = client;
        self
    }

    /// The `Publish` form for `message` to `to`.
    fn form(&self, to: &str, message: &SmsMessage) -> Vec<(String, String)> {
        let mut form = vec![
            ("Action".to_owned(), "Publish".to_owned()),
            ("Version".to_owned(), "2010-03-31".to_owned()),
            ("PhoneNumber".to_owned(), to.to_owned()),
            ("Message".to_owned(), message.body.clone()),
        ];
        let mut attributes = vec![("AWS.SNS.SMS.SMSType", self.sms_type.clone())];
        if let Some(from) = message.from.as_deref().or(self.from.as_deref()) {
            let name = if from.starts_with('+') {
                "AWS.MM.SMS.OriginationNumber"
            } else {
                "AWS.SNS.SMS.SenderID"
            };
            attributes.push((name, from.to_owned()));
        }
        for (i, (name, value)) in attributes.into_iter().enumerate() {
            let entry = format!("MessageAttributes.entry.{}", i + 1);
            form.push((format!("{entry}.Name"), name.to_owned()));
            form.push((format!("{entry}.Value.DataType"), "String".to_owned()));
            form.push((format!("{entry}.Value.StringValue"), value));
        }
        form
    }
}

/// The text of the first `<name>` element of `xml`.
fn tag(xml: &str, name: &str) -> Option<String> {
    let start = xml.find(&format!("<{name}>"))? + name.len() + 2;
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(unescape(&xml[start..end]))
}

/// The five XML entities.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

impl SmsDriver for SnsSms {
    fn send<'a>(
        &'a self,
        to: &'a str,
        message: &'a SmsMessage,
    ) -> BoxFuture<'a, Result<SentMessage, ChannelError>> {
        Box::pin(async move {
            let body = serde_urlencoded::to_string(self.form(to, message)).map_err(|error| {
                ChannelError::Config(format!("cannot encode the request: {error}"))
            })?;
            let mut url = self.base.clone();
            if url.path().is_empty() {
                url.set_path("/");
            }
            let signed = self.signer.sign(
                "POST",
                &url,
                &[("content-type", FORM)],
                body.as_bytes(),
                Utc::now(),
            );
            let mut request = self
                .client
                .post(url.as_str())
                .header("content-type", FORM)
                .body(body);
            for (name, value) in signed {
                request = request.header(name, value);
            }
            let response = request
                .send()
                .await
                .map_err(|error| unreachable(PROVIDER, error))?;
            let text = response.text();
            if !response.is_success() {
                return Err(ChannelError::Refused {
                    provider: PROVIDER.to_owned(),
                    status: response.status().as_u16(),
                    code: tag(&text, "Code"),
                    message: tag(&text, "Message")
                        .unwrap_or_else(|| "SNS refused the message".to_owned()),
                });
            }
            let sent = SentMessage::new(PROVIDER);
            Ok(match tag(&text, "MessageId") {
                Some(id) => sent.id(id),
                None => sent,
            })
        })
    }
}

impl fmt::Debug for SnsSms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnsSms")
            .field("region", &self.signer.region())
            .field("access_key_id", &self.signer.credentials().access_key_id)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elements_are_read_and_unescaped() {
        let xml = "<ErrorResponse><Error><Code>InvalidParameter</Code><Message>Invalid parameter: PhoneNumber &amp; more</Message></Error></ErrorResponse>";
        assert_eq!(tag(xml, "Code").as_deref(), Some("InvalidParameter"));
        assert_eq!(
            tag(xml, "Message").as_deref(),
            Some("Invalid parameter: PhoneNumber & more")
        );
        assert_eq!(tag(xml, "MessageId"), None);
    }
}
