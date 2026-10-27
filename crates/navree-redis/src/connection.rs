//! [`Redis`] and [`RedisConnection`]: the app's Redis connections.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use redis::aio::{ConnectionManager, ConnectionManagerConfig, MultiplexedConnection, PubSub};
use redis::{AsyncConnectionConfig, Client};

use crate::options::{DEFAULT_CONNECTION, redact_url};
use crate::{RedisError, RedisOptions};

/// How often the connection manager retries a lost connection before the
/// command that needed it fails (the next command tries again).
const RECONNECT_RETRIES: usize = 2;

/// The longest pause between two reconnection attempts.
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(1);

/// The app's Redis connections: the default one (`redis.url`) and the
/// named ones (`redis.connections.<name>`), plus the key prefix. Provided
/// and exported by `RedisModule`; cheap to share (`Arc<Redis>`).
///
/// Connections are opened lazily: the app boots while Redis is down, the
/// first command connects, and a lost connection is reopened on the next
/// command. The `redis` health check reports a server that does not answer.
///
/// ```no_run
/// use navree_redis::Redis;
/// use navree_redis::redis::AsyncCommands;
///
/// # async fn example(redis: &Redis) -> Result<(), navree_redis::RedisError> {
/// let mut conn = redis.manager();
/// let key = redis.key("greeting"); // "navree:greeting"
/// let _: () = conn.set_ex(&key, "hello", 60).await.map_err(|e| redis.error(e))?;
/// let greeting: Option<String> = conn.get(&key).await.map_err(|e| redis.error(e))?;
/// # let _ = greeting;
/// # Ok(())
/// # }
/// ```
pub struct Redis {
    default: RedisConnection,
    named: BTreeMap<String, RedisConnection>,
    prefix: String,
}

impl Redis {
    /// The connections `options` describe. Nothing connects yet; must run
    /// inside a Tokio runtime. Fails on invalid options.
    pub fn new(options: &RedisOptions) -> Result<Redis, RedisError> {
        options
            .check()
            .map_err(|errors| RedisError::Options(errors.join("; ")))?;
        let default = RedisConnection::open(DEFAULT_CONNECTION, &options.url, options)?;
        let named = options
            .connections
            .iter()
            .map(|(name, connection)| {
                RedisConnection::open(name, &connection.url, options)
                    .map(|connection| (name.clone(), connection))
            })
            .collect::<Result<_, _>>()?;
        Ok(Redis {
            default,
            named,
            prefix: options.prefix.clone(),
        })
    }

    /// The default connection (`redis.url`).
    pub fn connection(&self) -> &RedisConnection {
        &self.default
    }

    /// The connection `redis.connections.<name>` (`default` is `redis.url`).
    pub fn named(&self, name: &str) -> Result<&RedisConnection, RedisError> {
        if name == DEFAULT_CONNECTION {
            return Ok(&self.default);
        }
        self.named
            .get(name)
            .ok_or_else(|| RedisError::UnknownConnection {
                name: name.to_owned(),
            })
    }

    /// The connection named `name` when the configuration defines one, the
    /// default connection otherwise. The drivers pick theirs this way
    /// (`cache`, `queue`, `session`, `realtime`, `rate_limiter`).
    pub fn connection_for(&self, name: &str) -> &RedisConnection {
        self.named.get(name).unwrap_or(&self.default)
    }

    /// Every connection: the default one first, then the named ones by name.
    pub fn connections(&self) -> impl Iterator<Item = &RedisConnection> {
        std::iter::once(&self.default).chain(self.named.values())
    }

    /// A handle to the default connection's manager: multiplexed (cheap to
    /// clone, safe to share) and reconnecting. Use it with
    /// [`redis::AsyncCommands`].
    pub fn manager(&self) -> ConnectionManager {
        self.default.manager()
    }

    /// The key prefix (`redis.prefix`).
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// `key` with the prefix: `redis.key("greeting")` is `navree:greeting`.
    pub fn key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    /// A client error on the default connection as a [`RedisError`].
    pub fn error(&self, source: redis::RedisError) -> RedisError {
        self.default.error(source)
    }
}

impl fmt::Debug for Redis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redis")
            .field("default", &self.default)
            .field("named", &self.named.values().collect::<Vec<_>>())
            .field("prefix", &self.prefix)
            .finish()
    }
}

/// One Redis server: a lazily connected, reconnecting
/// [`ConnectionManager`] for ordinary commands, plus dedicated connections
/// for blocking commands and pub/sub. Cheap to clone.
#[derive(Clone)]
pub struct RedisConnection {
    inner: Arc<Inner>,
}

struct Inner {
    name: String,
    client: Client,
    manager: ConnectionManager,
    connect_timeout: Duration,
    response_timeout: Duration,
    redacted_url: String,
}

impl RedisConnection {
    /// The connection `name` to `url`, with the timeouts of `options`.
    fn open(name: &str, url: &str, options: &RedisOptions) -> Result<Self, RedisError> {
        let client = Client::open(url.trim()).map_err(|source| RedisError::command(name, source))?;
        let config = ConnectionManagerConfig::new()
            .set_connection_timeout(Some(options.connect_timeout()))
            .set_response_timeout(Some(options.response_timeout()))
            .set_number_of_retries(RECONNECT_RETRIES)
            .set_max_delay(RECONNECT_MAX_DELAY);
        let manager = ConnectionManager::new_lazy_with_config(client.clone(), config)
            .map_err(|source| RedisError::command(name, source))?;
        Ok(RedisConnection {
            inner: Arc::new(Inner {
                name: name.to_owned(),
                client,
                manager,
                connect_timeout: options.connect_timeout(),
                response_timeout: options.response_timeout(),
                redacted_url: redact_url(url),
            }),
        })
    }

    /// The connection's name: `default` for `redis.url`, else its key under
    /// `redis.connections`.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// A handle to the shared connection manager (cheap to clone): use it
    /// for ordinary commands with [`redis::AsyncCommands`].
    pub fn manager(&self) -> ConnectionManager {
        self.inner.manager.clone()
    }

    /// The client, to open other kinds of connections.
    pub fn client(&self) -> &Client {
        &self.inner.client
    }

    /// A new connection of its own, without a response timeout: for
    /// blocking commands (`BLPOP`, `XREAD BLOCK`) that would hold up the
    /// shared manager.
    pub async fn dedicated(&self) -> Result<MultiplexedConnection, RedisError> {
        let config = AsyncConnectionConfig::new()
            .set_connection_timeout(Some(self.inner.connect_timeout))
            .set_response_timeout(None);
        self.inner
            .client
            .get_multiplexed_async_connection_with_config(&config)
            .await
            .map_err(|source| self.error(source))
    }

    /// A new pub/sub connection.
    pub async fn pubsub(&self) -> Result<PubSub, RedisError> {
        tokio::time::timeout(self.inner.connect_timeout, self.inner.client.get_async_pubsub())
            .await
            .map_err(|_| {
                self.error(redis::RedisError::from(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "timed out connecting for pub/sub",
                )))
            })?
            .map_err(|source| self.error(source))
    }

    /// `PING`: whether the server answers, within the connect and response
    /// timeouts.
    pub async fn ping(&self) -> Result<(), RedisError> {
        let mut manager = self.manager();
        let limit = self.inner.connect_timeout + self.inner.response_timeout;
        let answer: Result<String, redis::RedisError> =
            match tokio::time::timeout(limit, redis::cmd("PING").query_async(&mut manager)).await {
                Ok(answer) => answer,
                Err(_) => Err(redis::RedisError::from(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "no answer to PING in time",
                ))),
            };
        answer.map(drop).map_err(|source| self.error(source))
    }

    /// A client error on this connection as a [`RedisError`].
    pub fn error(&self, source: redis::RedisError) -> RedisError {
        RedisError::command(&self.inner.name, source)
    }
}

impl fmt::Debug for RedisConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisConnection")
            .field("name", &self.inner.name)
            .field("url", &self.inner.redacted_url)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RedisConnectionOptions;

    fn options() -> RedisOptions {
        let mut options = RedisOptions {
            url: "redis://:secret@127.0.0.1:1/0".to_owned(),
            prefix: "crm:".to_owned(),
            connect_timeout_ms: 200,
            response_timeout_ms: 200,
            ..RedisOptions::default()
        };
        options.connections.insert(
            "queue".to_owned(),
            RedisConnectionOptions {
                url: "redis://127.0.0.1:2".to_owned(),
            },
        );
        options
    }

    #[tokio::test]
    async fn connections_are_opened_lazily_and_found_by_name() {
        let redis = Redis::new(&options()).expect("no connection is made");
        assert_eq!(redis.connection().name(), "default");
        assert_eq!(redis.named("default").unwrap().name(), "default");
        assert_eq!(redis.named("queue").unwrap().name(), "queue");
        assert_eq!(redis.connection_for("queue").name(), "queue");
        assert_eq!(redis.connection_for("cache").name(), "default");
        assert_eq!(
            redis.connections().map(RedisConnection::name).collect::<Vec<_>>(),
            ["default", "queue"]
        );
        let error = redis.named("cache").unwrap_err().to_string();
        assert!(error.contains("redis.connections.cache.url"), "{error}");
        assert_eq!(redis.prefix(), "crm:");
        assert_eq!(redis.key("greeting"), "crm:greeting");
        let debug = format!("{redis:?}");
        assert!(!debug.contains("secret") && debug.contains("***"), "{debug}");
    }

    #[tokio::test]
    async fn a_server_that_does_not_answer_fails_the_command_not_the_boot() {
        let redis = Redis::new(&options()).unwrap();
        let error = redis.connection().ping().await.unwrap_err();
        let message = error.to_string();
        assert!(message.starts_with("Redis connection `default`"), "{message}");
        assert!(!message.contains("secret"), "{message}");
        assert!(redis.connection().pubsub().await.is_err());
        assert!(redis.connection().dedicated().await.is_err());
    }

    #[tokio::test]
    async fn invalid_options_are_refused() {
        let error = Redis::new(&RedisOptions {
            url: "http://example.com".to_owned(),
            ..RedisOptions::default()
        })
        .unwrap_err();
        assert!(matches!(error, RedisError::Options(_)), "{error}");
    }
}
