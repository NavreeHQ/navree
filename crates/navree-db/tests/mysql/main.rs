//! navree-db against a real MySQL server (the `mysql` feature).
//!
//! Set `NAVREE_TEST_MYSQL_URL` to a server URL whose user may create and
//! drop databases (`mysql://root:secret@127.0.0.1:3306/navree_test`); every
//! test works in a scratch database of its own. Without the variable each
//! test prints `SKIPPED` and passes, unless `NAVREE_TEST_REQUIRE_SERVICES=1`.
//!
//! ```sh
//! NAVREE_TEST_MYSQL_URL=mysql://root:secret@127.0.0.1:3306/navree_test \
//!     cargo test -p navree-db --features mysql --test mysql
//! ```

mod fixtures;
mod support;

mod connect;
mod errors;
mod health;
mod migrations;
mod monitor;
mod paginate;
mod repo;
mod text;
mod time_series;
mod uow;
