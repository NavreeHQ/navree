//! `MailOptions` and `TransportConfig`: section `mail`, shorthands, validation
//! and building transports.

use std::any::Any;
use std::sync::Arc;

use navree_config::{Config, ConfigError, ConfigSources};
use navree_mail::{
    ArrayTransport, FailoverTransport, LogTransport, MailError, MailOptions, MailTransport,
    SmtpTransport, TransportConfig,
};
use serde_json::{Value, json};

fn load(sources: ConfigSources) -> Result<MailOptions, ConfigError> {
    Config::load(&sources)?.typed::<MailOptions>()
}

fn parse(section: Value) -> Result<MailOptions, ConfigError> {
    load(ConfigSources::new().set("mail", section))
}

fn transport(section: Value) -> TransportConfig {
    parse(json!({ "transport": section }))
        .expect("valid transport")
        .transport
}

fn is<T: MailTransport>(transport: &Arc<dyn MailTransport>) -> bool {
    (transport.as_ref() as &dyn Any).is::<T>()
}

#[test]
fn options_default_to_the_log_transport() {
    let options = load(ConfigSources::new()).expect("defaults");

    assert_eq!(options, MailOptions::default());
    assert_eq!(options.transport, TransportConfig::Log);
    assert_eq!(options.from, "hello@example.com");
    assert_eq!(options.queue, None);
}

#[test]
fn transport_config_reads_tables() {
    assert_eq!(
        transport(json!({ "type": "smtp", "url": "smtps://u:p@smtp.example.com" })),
        TransportConfig::smtp("smtps://u:p@smtp.example.com")
    );
    assert_eq!(transport(json!({ "type": "log" })), TransportConfig::Log);
    assert_eq!(
        transport(json!({ "type": "array" })),
        TransportConfig::Array
    );
    assert_eq!(
        transport(json!({
            "type": "failover",
            "transports": [{ "type": "smtp", "url": "smtp://localhost:1025" }, "log"],
        })),
        TransportConfig::Failover(vec![
            TransportConfig::smtp("smtp://localhost:1025"),
            TransportConfig::Log,
        ])
    );
}

#[test]
fn transport_config_reads_shorthand_strings() {
    assert_eq!(transport(json!("log")), TransportConfig::Log);
    assert_eq!(transport(json!("array")), TransportConfig::Array);
    assert_eq!(
        transport(json!("smtp://localhost:1025")),
        TransportConfig::smtp("smtp://localhost:1025")
    );
    assert_eq!(
        transport(json!("smtps://u:p@smtp.example.com")),
        TransportConfig::smtp("smtps://u:p@smtp.example.com")
    );
    assert_eq!(
        transport(json!({ "type": "failover", "transports": "smtp://localhost:1025, log" })),
        TransportConfig::Failover(vec![
            TransportConfig::smtp("smtp://localhost:1025"),
            TransportConfig::Log,
        ])
    );
}

#[test]
fn options_read_environment_variables() {
    let options = load(ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_MAIL__FROM", "Acme <billing@acme.test>"),
            ("APP_MAIL__TRANSPORT", "smtp://localhost:1025"),
            ("APP_MAIL__QUEUE", "mail"),
        ],
    ))
    .expect("env");

    assert_eq!(
        options,
        MailOptions {
            from: "Acme <billing@acme.test>".to_owned(),
            transport: TransportConfig::smtp("smtp://localhost:1025"),
            queue: Some("mail".to_owned()),
            ..MailOptions::default()
        }
    );

    let failover = load(ConfigSources::new().env_vars(
        "APP_",
        [
            ("APP_MAIL__TRANSPORT__TYPE", "failover"),
            (
                "APP_MAIL__TRANSPORT__TRANSPORTS",
                "smtp://localhost:1025,log",
            ),
        ],
    ))
    .expect("env failover");
    assert_eq!(
        failover.transport,
        TransportConfig::Failover(vec![
            TransportConfig::smtp("smtp://localhost:1025"),
            TransportConfig::Log,
        ])
    );
}

#[test]
fn invalid_options_fail_without_echoing_secrets() {
    for section in [
        json!({ "transport": { "type": "smpt", "url": "smtp://u:s3cret@smtp.example.com" } }),
        json!({ "transport": "smpt://u:s3cret@smtp.example.com" }),
        json!({ "transport": { "type": "smtp", "url": "smtp://u:s3cret@smtp.example.com?tls=maybe" } }),
        json!({ "transport": { "type": "failover", "transports": [] } }),
        json!({ "from": "not an address" }),
    ] {
        let error = parse(section.clone()).expect_err(&section.to_string());
        let message = error.to_string();
        assert!(message.contains("mail"), "{message}");
        assert!(!message.contains("s3cret"), "{message}");
    }
}

#[test]
fn debug_redacts_the_smtp_url() {
    let debug = format!(
        "{:?}",
        MailOptions {
            transport: TransportConfig::smtp("smtps://u:s3cret@smtp.example.com"),
            ..MailOptions::default()
        }
    );

    assert!(!debug.contains("s3cret"), "{debug}");
}

#[test]
fn transport_config_builds_transports() {
    assert!(is::<LogTransport>(
        &TransportConfig::Log.build().expect("log")
    ));
    assert!(is::<ArrayTransport>(
        &TransportConfig::Array.build().expect("array")
    ));
    assert!(is::<SmtpTransport>(
        &TransportConfig::smtp("smtp://localhost:1025")
            .build()
            .expect("smtp")
    ));
    assert!(is::<FailoverTransport>(
        &TransportConfig::Failover(vec![TransportConfig::Log])
            .build()
            .expect("failover")
    ));

    let error = TransportConfig::smtp("ftp://localhost")
        .build()
        .expect_err("bad scheme");
    assert!(matches!(error, MailError::Config(_)), "{error}");
    let error = TransportConfig::Failover(Vec::new())
        .build()
        .expect_err("empty failover");
    assert!(matches!(error, MailError::Config(_)), "{error}");
}
