//! [`RedisError`].

/// What can go wrong talking to Redis through navree-redis.
///
/// Messages never contain a URL (which may hold a password) or a stored
/// value.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RedisError {
    /// [`Redis::named`](crate::Redis::named) was asked for a connection the
    /// configuration does not define.
    #[error("there is no Redis connection named `{name}`; add redis.connections.{name}.url")]
    UnknownConnection {
        /// The name asked for.
        name: String,
    },
    /// The options are invalid (see [`RedisOptions::check`](crate::RedisOptions::check)).
    #[error("invalid Redis options: {0}")]
    Options(String),
    /// A command or connection failed on the connection `connection`.
    #[error("Redis connection `{connection}`: {source}")]
    Command {
        /// The connection's name (`default` for `redis.url`).
        connection: String,
        /// The client's error.
        #[source]
        source: redis::RedisError,
    },
    /// A value read from Redis is not what the driver stored.
    #[error("unexpected data under Redis key `{key}`: {message}")]
    Data {
        /// The key (prefix included).
        key: String,
        /// What is wrong.
        message: String,
    },
}

impl RedisError {
    /// A failed command on `connection`.
    pub(crate) fn command(connection: &str, source: redis::RedisError) -> Self {
        RedisError::Command {
            connection: connection.to_owned(),
            source,
        }
    }
}
