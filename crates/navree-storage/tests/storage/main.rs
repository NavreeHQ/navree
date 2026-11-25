//! navree-storage integration tests: the disk conformance suite on every
//! driver, the signed download and upload routes, presigned uploads, disk
//! sync, configuration, the upload helper and the public store.

mod support;

mod cloud;
mod conformance;
mod disk;
mod drivers;
mod emulators;
mod local;
mod local_hardening;
mod module;
mod options;
mod presign;
#[cfg(any(feature = "s3", feature = "gcs"))]
mod s3_server;
mod serve;
mod store;
mod sync;
mod telemetry;
mod upload;
mod urls;
