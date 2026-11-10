//! navree-schema against real servers: every statement the grammars write,
//! run by PostgreSQL (with TimescaleDB) and MySQL, read back, dumped and
//! loaded again.
//!
//! Opt-in. Build with the features of the servers to test and point the
//! variables at throwaway servers, as a user that may create databases:
//!
//! ```text
//! NAVREE_TEST_POSTGRES_URL=postgres://postgres:secret@127.0.0.1:55432/postgres \
//! NAVREE_TEST_MYSQL_URL=mysql://root:secret@127.0.0.1:53306/test \
//! cargo test -p navree-schema --features postgres,mysql --test servers
//! ```
//!
//! Each test creates its own database and drops it at the end; a test
//! whose variable is unset says so and passes.

mod support;

#[cfg(feature = "mysql")]
mod mysql;
#[cfg(feature = "postgres")]
mod postgres;
