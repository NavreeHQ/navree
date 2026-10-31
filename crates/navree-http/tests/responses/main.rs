//! Responses, bodies and the app stack: what a response is made of, how a
//! body streams and fails, how layers wrap an app, and what a client reads
//! off the wire. The long lists (the app stack, the wire) are pinned in files
//! of `golden/`: `NAVREE_BLESS=1 cargo test -p navree-http --test responses`
//! writes them again, and the diff says what changed.

mod bodies;
mod shapes;
mod sse;
mod stack;
mod support;
mod wire;
