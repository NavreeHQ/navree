//! navree-observe's logs for operators, in a test binary of their own.
//!
//! `tracing` remembers, for each place that logs and for the whole process, whether any subscriber wants what it
//! logs. While only one subscriber is registered, the first thread to reach that place decides alone, by its own
//! subscriber, and a thread with none decides that nobody does. These tests listen with a subscriber for their own
//! thread, so a test beside them that reached the same place first with none (a health check that is down, a
//! metrics token refused) would hide from them the very lines they look for. Here every test sets its subscriber
//! before it runs anything that logs.

#[path = "../observe/support.rs"]
mod support;

mod logs;
