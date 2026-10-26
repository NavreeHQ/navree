//! [`CaptureWriter`]: collects log output in memory, for tests.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use tracing_subscriber::fmt::MakeWriter;

/// A writer that keeps everything written to it in memory.
#[derive(Clone, Debug, Default)]
pub struct CaptureWriter {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl CaptureWriter {
    /// An empty capture.
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything written so far, as (lossy) UTF-8.
    pub fn contents(&self) -> String {
        let buf = self.buf.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&buf).into_owned()
    }
}

impl io::Write for CaptureWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CaptureWriter {
    type Writer = CaptureWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
