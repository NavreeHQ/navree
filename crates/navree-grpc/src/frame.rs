//! Length-prefixed messages: gRPC's framing, which Connect's streaming
//! envelopes share (one flag byte, a big-endian `u32` length, the message).

use bytes::{Buf, BufMut, Bytes, BytesMut};

/// The header length of a frame.
pub(crate) const HEADER: usize = 5;

/// A frame with `flag` around `payload`.
pub(crate) fn frame(flag: u8, payload: &[u8]) -> Bytes {
    let mut out = BytesMut::with_capacity(HEADER + payload.len());
    out.put_u8(flag);
    out.put_u32(u32::try_from(payload.len()).unwrap_or(u32::MAX));
    out.put_slice(payload);
    out.freeze()
}

/// Splits a byte stream into frames.
#[derive(Debug, Default)]
pub(crate) struct Frames {
    buffer: BytesMut,
}

impl Frames {
    /// Appends received bytes.
    pub(crate) fn push(&mut self, data: impl Buf) {
        self.buffer.put(data);
    }

    /// The next complete frame: its flag and payload.
    pub(crate) fn next_frame(&mut self) -> Option<(u8, Bytes)> {
        if self.buffer.len() < HEADER {
            return None;
        }
        let flag = self.buffer[0];
        let length = u32::from_be_bytes([
            self.buffer[1],
            self.buffer[2],
            self.buffer[3],
            self.buffer[4],
        ]) as usize;
        if self.buffer.len() < HEADER + length {
            return None;
        }
        self.buffer.advance(HEADER);
        Some((flag, self.buffer.split_to(length).freeze()))
    }

    /// Bytes received but not yet part of a complete frame.
    pub(crate) fn pending(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_across_chunk_boundaries() {
        let mut stream = BytesMut::new();
        stream.put(frame(0, b"hello"));
        stream.put(frame(2, b"{}"));
        stream.put(frame(0, b""));
        let bytes = stream.freeze();
        let mut frames = Frames::default();
        let mut seen = Vec::new();
        for chunk in bytes.chunks(3) {
            frames.push(chunk);
            while let Some(frame) = frames.next_frame() {
                seen.push(frame);
            }
        }
        assert_eq!(
            seen,
            [
                (0, Bytes::from_static(b"hello")),
                (2, Bytes::from_static(b"{}")),
                (0, Bytes::new()),
            ]
        );
        assert_eq!(frames.pending(), 0);
    }
}
