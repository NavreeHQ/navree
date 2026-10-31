//! [`Frames`]: what an HTTP/2 client's bytes say, for the upgraded idle
//! timeout.
//!
//! Keep-alive is a connection talking to itself: a PING says nothing about
//! whether anyone is on an upgraded stream, and neither do the other frames
//! that look after a connection. Only `DATA` is the stream's own traffic. The
//! frames are followed from the start of the connection, because a read can
//! end anywhere in one and hold any number of them.

/// What opens an HTTP/2 connection, before its first frame.
const PREFACE_BYTES: usize = 24;

const HEADER_BYTES: usize = 9;
const LENGTH_BYTES: usize = 3;
const TYPE_AT: usize = 3;
const DATA: u8 = 0;

/// See the [module documentation](self).
#[derive(Debug)]
pub(super) struct Frames {
    /// What is left of the preface or of a frame's payload.
    skip: usize,
    /// Whether that is `DATA`.
    in_data: bool,
    /// The header being read.
    header: [u8; HEADER_BYTES],
    have: usize,
}

impl Frames {
    pub(super) fn new() -> Self {
        Self {
            skip: PREFACE_BYTES,
            in_data: false,
            header: [0; HEADER_BYTES],
            have: 0,
        }
    }

    /// Follows `bytes`, the next read: whether any of them was stream data.
    pub(super) fn read_data(&mut self, mut bytes: &[u8]) -> bool {
        let mut data = false;
        loop {
            let skipped = self.skip.min(bytes.len());
            self.skip -= skipped;
            data |= self.in_data && skipped > 0;
            bytes = &bytes[skipped..];
            if bytes.is_empty() {
                return data;
            }

            let taken = (HEADER_BYTES - self.have).min(bytes.len());
            self.header[self.have..self.have + taken].copy_from_slice(&bytes[..taken]);
            self.have += taken;
            bytes = &bytes[taken..];
            if self.have < HEADER_BYTES {
                return data;
            }

            self.have = 0;
            let mut length = [0; size_of::<u32>()];
            length[size_of::<u32>() - LENGTH_BYTES..].copy_from_slice(&self.header[..LENGTH_BYTES]);
            self.skip = u32::from_be_bytes(length) as usize;
            self.in_data = self.header[TYPE_AT] == DATA;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PING: u8 = 6;
    const SETTINGS: u8 = 4;
    const WINDOW_UPDATE: u8 = 8;

    fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = (payload.len() as u32).to_be_bytes()[1..].to_vec();
        frame.extend_from_slice(&[kind, 0, 0, 0, 0, 0]);
        frame.extend_from_slice(payload);
        frame
    }

    /// Past the preface.
    fn frames() -> Frames {
        let mut frames = Frames::new();
        assert!(!frames.read_data(&[0; PREFACE_BYTES]));
        frames
    }

    #[test]
    fn the_preface_says_nothing() {
        let mut frames = Frames::new();
        assert!(!frames.read_data(&[0; PREFACE_BYTES - 1]));
        assert!(!frames.read_data(&frame(PING, &[0; 8])[..1]));
    }

    #[test]
    fn frames_that_look_after_the_connection_are_silence_however_many_arrive_at_once() {
        let mut read = frame(PING, &[0; 8]);
        read.extend(frame(PING, &[1; 8]));
        read.extend(frame(SETTINGS, &[]));
        read.extend(frame(WINDOW_UPDATE, &[0, 0, 0, 1]));
        assert!(!frames().read_data(&read));
    }

    #[test]
    fn data_is_traffic_wherever_it_is_in_a_read() {
        let mut read = frame(PING, &[0; 8]);
        read.extend(frame(DATA, b"hello"));
        read.extend(frame(PING, &[0; 8]));
        assert!(frames().read_data(&read));
    }

    /// A flood of these keeps nothing open: they carry nothing.
    #[test]
    fn data_with_nothing_in_it_is_silence() {
        assert!(!frames().read_data(&frame(DATA, &[])));
    }

    /// A read ends anywhere: a split must find neither more nor less.
    #[test]
    fn frames_split_across_reads_say_the_same() {
        let mut quiet = frame(PING, &[6; 8]);
        quiet.extend(frame(WINDOW_UPDATE, &[0, 0, 0, 1]));
        let mut talk = quiet.clone();
        talk.extend(frame(DATA, b"hello"));
        for split in 0..quiet.len() {
            let mut frames = frames();
            assert!(!frames.read_data(&quiet[..split]), "at {split}");
            assert!(!frames.read_data(&quiet[split..]), "at {split}");
        }
        for split in 0..talk.len() {
            let mut frames = frames();
            let spoke = frames.read_data(&talk[..split]) | frames.read_data(&talk[split..]);
            assert!(spoke, "at {split}");
        }
    }

    /// The bytes of one frame arriving slowly are all traffic.
    #[test]
    fn each_read_of_a_long_data_frame_is_traffic() {
        let frame = frame(DATA, &[0; 64]);
        let mut frames = frames();
        assert!(frames.read_data(&frame[..HEADER_BYTES + 1]));
        assert!(frames.read_data(&frame[HEADER_BYTES + 1..HEADER_BYTES + 2]));
        assert!(frames.read_data(&frame[HEADER_BYTES + 2..]));
    }

    /// Bytes inside a payload are not frames, whatever they look like.
    #[test]
    fn a_payload_is_never_read_as_frames() {
        let inner = frame(DATA, b"hello");
        assert!(!frames().read_data(&frame(PING, &inner)));
    }
}
