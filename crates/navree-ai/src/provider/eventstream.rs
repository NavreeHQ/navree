//! The AWS event stream encoding (`application/vnd.amazon.eventstream`),
//! which Bedrock's `ConverseStream` answers in: length-prefixed binary
//! messages with typed headers and a CRC-32 at each end.

use std::collections::BTreeMap;

use bytes::{Buf, BytesMut};

/// One decoded message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EventMessage {
    /// String headers (`:event-type`, `:message-type`, `:exception-type`).
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) payload: Vec<u8>,
}

impl EventMessage {
    /// A header's value.
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }
}

/// An incremental decoder.
#[derive(Debug, Default)]
pub(crate) struct Decoder {
    buffer: BytesMut,
}

impl Decoder {
    /// Feeds bytes; returns the messages they complete, or an error for a
    /// corrupt stream.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Result<Vec<EventMessage>, String> {
        self.buffer.extend_from_slice(bytes);
        let mut messages = Vec::new();
        loop {
            if self.buffer.len() < 12 {
                return Ok(messages);
            }
            let total = u32::from_be_bytes(self.buffer[0..4].try_into().expect("4 bytes")) as usize;
            if total < 16 {
                return Err(format!("event stream message too short ({total} bytes)"));
            }
            if self.buffer.len() < total {
                return Ok(messages);
            }
            let frame = self.buffer.split_to(total);
            messages.push(decode(&frame)?);
        }
    }
}

/// One whole message.
fn decode(frame: &[u8]) -> Result<EventMessage, String> {
    let total = frame.len();
    let headers_len = u32::from_be_bytes(frame[4..8].try_into().expect("4 bytes")) as usize;
    let prelude_crc = u32::from_be_bytes(frame[8..12].try_into().expect("4 bytes"));
    if crc32(&frame[0..8]) != prelude_crc {
        return Err("event stream prelude checksum mismatch".to_owned());
    }
    let message_crc = u32::from_be_bytes(frame[total - 4..].try_into().expect("4 bytes"));
    if crc32(&frame[..total - 4]) != message_crc {
        return Err("event stream message checksum mismatch".to_owned());
    }
    if 12 + headers_len > total - 4 {
        return Err("event stream headers overflow the message".to_owned());
    }
    let mut headers_bytes = &frame[12..12 + headers_len];
    let mut headers = BTreeMap::new();
    while headers_bytes.has_remaining() {
        let name_len = headers_bytes.get_u8() as usize;
        if headers_bytes.remaining() < name_len + 1 {
            return Err("truncated event stream header".to_owned());
        }
        let name = String::from_utf8_lossy(&headers_bytes[..name_len]).into_owned();
        headers_bytes.advance(name_len);
        let kind = headers_bytes.get_u8();
        let skip = |bytes: &mut &[u8], n: usize| -> Result<(), String> {
            if bytes.remaining() < n {
                return Err("truncated event stream header value".to_owned());
            }
            bytes.advance(n);
            Ok(())
        };
        match kind {
            0 | 1 => {}
            2 => skip(&mut headers_bytes, 1)?,
            3 => skip(&mut headers_bytes, 2)?,
            4 => skip(&mut headers_bytes, 4)?,
            5 | 8 => skip(&mut headers_bytes, 8)?,
            9 => skip(&mut headers_bytes, 16)?,
            6 | 7 => {
                if headers_bytes.remaining() < 2 {
                    return Err("truncated event stream header value".to_owned());
                }
                let len = headers_bytes.get_u16() as usize;
                if headers_bytes.remaining() < len {
                    return Err("truncated event stream header value".to_owned());
                }
                if kind == 7 {
                    headers.insert(name, String::from_utf8_lossy(&headers_bytes[..len]).into_owned());
                }
                headers_bytes.advance(len);
            }
            other => return Err(format!("unknown event stream header type {other}")),
        }
    }
    Ok(EventMessage {
        headers,
        payload: frame[12 + headers_len..total - 4].to_vec(),
    })
}

/// CRC-32 (IEEE 802.3).
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Encodes a message with string headers (tests and fixtures).
#[cfg(test)]
pub(crate) fn encode(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
    let mut header_bytes = Vec::new();
    for (name, value) in headers {
        header_bytes.push(name.len() as u8);
        header_bytes.extend_from_slice(name.as_bytes());
        header_bytes.push(7);
        header_bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
        header_bytes.extend_from_slice(value.as_bytes());
    }
    let total = 12 + header_bytes.len() + payload.len() + 4;
    let mut frame = Vec::with_capacity(total);
    frame.extend_from_slice(&(total as u32).to_be_bytes());
    frame.extend_from_slice(&(header_bytes.len() as u32).to_be_bytes());
    let prelude_crc = crc32(&frame);
    frame.extend_from_slice(&prelude_crc.to_be_bytes());
    frame.extend_from_slice(&header_bytes);
    frame.extend_from_slice(payload);
    let crc = crc32(&frame);
    frame.extend_from_slice(&crc.to_be_bytes());
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn decodes_messages_split_across_chunks() {
        let mut bytes = encode(
            &[(":event-type", "contentBlockDelta"), (":message-type", "event")],
            br#"{"delta":{"text":"Hi"}}"#,
        );
        bytes.extend(encode(&[(":event-type", "messageStop")], br#"{"stopReason":"end_turn"}"#));
        let mut decoder = Decoder::default();
        let (first, rest) = bytes.split_at(20);
        assert!(decoder.feed(first).unwrap().is_empty());
        let messages = decoder.feed(rest).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].header(":event-type"), Some("contentBlockDelta"));
        assert_eq!(messages[0].payload, br#"{"delta":{"text":"Hi"}}"#);
        assert_eq!(messages[1].header(":event-type"), Some("messageStop"));
    }

    #[test]
    fn corrupt_messages_fail() {
        let mut bytes = encode(&[(":event-type", "x")], b"{}");
        let last = bytes.len() - 5;
        bytes[last] ^= 0xFF;
        assert!(Decoder::default().feed(&bytes).is_err());
    }
}
