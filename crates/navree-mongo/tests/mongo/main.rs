//! navree-mongo integration tests. The command tests (`commands`,
//! `schema_commands`) and the module wiring run without a server; the
//! others run against `NAVREE_TEST_MONGODB_URL` and pass with a notice when
//! it is not set (see `support`).
