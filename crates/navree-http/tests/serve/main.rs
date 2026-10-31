//! `navree_http::serve` against real sockets, with a client that misbehaves
//! on purpose. Limits are configured short and every bound on how late
//! something may happen is generous, so a loaded machine slows these tests
//! down but cannot fail them.

mod attacks;
mod connections;
mod http2;
mod requests;
mod shutdown;
mod slow_clients;
mod slow_readers;
mod support;
mod websockets;
