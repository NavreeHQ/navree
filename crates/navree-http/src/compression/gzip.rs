//! [`GzipBody`]: a body gzipped as it streams.

use std::io;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use bytes::{Buf, Bytes};
use flate2::{Compress, Compression, Crc, FlushCompress, Status};
use http::HeaderMap;
use http_body::{Body as HttpBody, Frame, SizeHint};
use navree_kernel::BoxError;
use pin_project_lite::pin_project;

/// Frames are cut from buffers of this size.
const BUFFER: usize = 4096;

/// The fixed gzip header: deflate, no name or time, level byte 0 (neither
/// the fastest nor the best level), unknown system.
const HEADER: [u8; 10] = [0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 0xff];

/// The compression level: zlib's default.
const LEVEL: u32 = 6;

pin_project! {
    /// A response body gzipped as it streams, from
    /// [`CompressionLayer`](super::CompressionLayer).
    ///
    /// - **Frames** are at most 4 KiB: each takes what is left of a 4 KiB
    ///   buffer, and a new buffer starts when one is used up. The inner body
    ///   is read only while the current frame has room, so a fast body is
    ///   not read ahead of a slow client: the memory held is the
    ///   compressor's (about 256 KiB at level 6), the chunk being read and
    ///   one frame.
    /// - **Flushes**: when the inner body has nothing ready, everything it
    ///   handed over is flushed (a deflate sync flush) and sent, so a slow
    ///   stream reaches the client as it goes. After a full frame, what was
    ///   read is flushed before reading more, so data never waits in the
    ///   compressor for more than one frame.
    /// - **Trailers** of the inner body follow the gzip stream. An error of
    ///   the inner body ends this one with that error.
    /// - **The end**: once the inner body has ended, it is never polled
    ///   again (a stream may panic if it is).
    /// - **Size**: unknown until the end.
    pub(crate) struct GzipBody<B> {
        #[pin]
        body: B,
        // What is left of the last data the body handed over.
        chunk: Bytes,
        // The body has handed over all its data (its end or its trailers).
        drained: bool,
        // The body has ended, or this one has after its trailers: the body
        // is not polled again.
        ended: bool,
        trailers: Option<HeaderMap>,
        encoder: Encoder,
        // The room left in the current buffer.
        room: usize,
        // The gzip stream is complete and sent.
        done: bool,
    }
}

impl<B> GzipBody<B> {
    /// Gzips `body`.
    pub(crate) fn new(body: B) -> Self {
        Self {
            body,
            chunk: Bytes::new(),
            drained: false,
            ended: false,
            trailers: None,
            encoder: Encoder::new(),
            room: BUFFER,
            done: false,
        }
    }
}

impl<B> HttpBody for GzipBody<B>
where
    B: HttpBody<Data = Bytes>,
    B::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let mut this = self.project();
        if !*this.done {
            if *this.room == 0 {
                *this.room = BUFFER;
            }
            let mut out = Out::new(*this.room);
            let mut source = Source {
                body: this.body.as_mut(),
                chunk: this.chunk,
                drained: this.drained,
                ended: this.ended,
                trailers: this.trailers,
            };
            match ready!(this.encoder.poll(&mut source, cx, &mut out)) {
                Ok(()) if out.len == 0 => *this.done = true,
                Ok(()) => {
                    *this.room -= out.len;
                    return Poll::Ready(Some(Ok(Frame::data(out.into_bytes()))));
                }
                Err(Failure::Body(error)) => return Poll::Ready(Some(Err(error))),
                Err(Failure::Deflate(error)) => return Poll::Ready(Some(Err(error.into()))),
            }
        }
        if let Some(trailers) = this.trailers.take() {
            return Poll::Ready(Some(Ok(Frame::trailers(trailers))));
        }
        if *this.ended {
            return Poll::Ready(None);
        }
        // Whatever the body sends after its trailers: more trailers pass,
        // data cannot be added to a finished gzip stream.
        let next = match ready!(this.body.poll_frame(cx)) {
            Some(Ok(frame)) => match frame.into_data() {
                Ok(data) if data.has_remaining() => Some(Err(
                    "the body sent data after the end of its gzip stream".into(),
                )),
                Ok(_) => None,
                Err(frame) => Some(Ok(frame)),
            },
            Some(Err(error)) => Some(Err(error.into())),
            None => None,
        };
        *this.ended = next.is_none();
        Poll::Ready(next)
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::new()
    }

    fn is_end_stream(&self) -> bool {
        false
    }
}

/// Why the gzip stream stopped.
enum Failure {
    /// The inner body failed: its error, as it is.
    Body(BoxError),
    /// The compressor failed.
    Deflate(io::Error),
}

/// The inner body read as a buffer of bytes: [`fill`](Self::fill) holds
/// its next chunk.
struct Source<'a, B> {
    body: Pin<&'a mut B>,
    chunk: &'a mut Bytes,
    drained: &'a mut bool,
    ended: &'a mut bool,
    trailers: &'a mut Option<HeaderMap>,
}

impl<B> Source<'_, B>
where
    B: HttpBody<Data = Bytes>,
    B::Error: Into<BoxError>,
{
    /// Makes `chunk` hold data, unless the body has no more (then it is
    /// empty). Empty data frames are skipped; trailers end the data.
    fn fill(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BoxError>> {
        while self.chunk.is_empty() && !*self.drained {
            match ready!(self.body.as_mut().poll_frame(cx)) {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => *self.chunk = data,
                    Err(frame) => {
                        *self.drained = true;
                        *self.trailers = frame.into_trailers().ok();
                    }
                },
                Some(Err(error)) => return Poll::Ready(Err(error.into())),
                None => {
                    *self.drained = true;
                    *self.ended = true;
                }
            }
        }
        Poll::Ready(Ok(()))
    }
}

/// A frame being written: `len` bytes of `buf` are used.
struct Out {
    buf: Vec<u8>,
    len: usize,
}

impl Out {
    fn new(room: usize) -> Self {
        Self {
            buf: vec![0; room],
            len: 0,
        }
    }

    fn is_full(&self) -> bool {
        self.len == self.buf.len()
    }

    /// Copies as much of `bytes` as fits; returns how much.
    fn copy(&mut self, bytes: &[u8]) -> usize {
        let free = &mut self.buf[self.len..];
        let n = free.len().min(bytes.len());
        free[..n].copy_from_slice(&bytes[..n]);
        self.len += n;
        n
    }

    /// Runs `deflate` on `input` into the free room; returns its status and
    /// how much of `input` it took.
    fn deflate(
        &mut self,
        deflate: &mut Compress,
        input: &[u8],
        flush: FlushCompress,
    ) -> io::Result<(Status, usize)> {
        let (taken, written) = (deflate.total_in(), deflate.total_out());
        let status = deflate
            .compress(input, &mut self.buf[self.len..], flush)
            .map_err(io::Error::other)?;
        let taken = usize::try_from(deflate.total_in() - taken).map_err(io::Error::other)?;
        let written = usize::try_from(deflate.total_out() - written).map_err(io::Error::other)?;
        self.len += written;
        Ok((status, taken))
    }

    fn into_bytes(mut self) -> Bytes {
        self.buf.truncate(self.len);
        Bytes::from(self.buf)
    }
}

/// Where the encoder is between two calls.
enum Step {
    /// Taking input; `read` bytes since the last flush.
    Encoding {
        read: usize,
    },
    /// A flush was cut short by a full frame.
    Flushing,
    /// The input has ended; the rest of the stream is being written.
    Finishing,
    Done,
    /// The compressor failed after writing part of a frame: the error comes
    /// with the next call.
    Failed(io::Error),
}

/// The part of the gzip stream being written.
enum Part {
    /// The header, `n` bytes of it written.
    Header(usize),
    Deflate,
    /// The CRC-32 and size trailer, `n` bytes of it written.
    Footer([u8; 8], usize),
    Done,
}

/// The gzip stream: header, raw deflate and footer, written into frames.
struct Encoder {
    deflate: Compress,
    crc: Crc,
    part: Part,
    // Nothing is held back in the compressor since its last flush.
    flushed: bool,
    step: Step,
}

impl Encoder {
    fn new() -> Self {
        Self {
            deflate: Compress::new(Compression::new(LEVEL), false),
            crc: Crc::new(),
            part: Part::Header(0),
            flushed: true,
            step: Step::Encoding { read: 0 },
        }
    }

    /// Fills `out` from `source`: ready with `out` full, with the stream
    /// ended, or when the source has nothing ready and `out` holds a
    /// flushed frame; pending with `out` empty.
    fn poll<B>(
        &mut self,
        source: &mut Source<'_, B>,
        cx: &mut Context<'_>,
        out: &mut Out,
    ) -> Poll<Result<(), Failure>>
    where
        B: HttpBody<Data = Bytes>,
        B::Error: Into<BoxError>,
    {
        // First what is owed from the last frame: a flush, or the end.
        if let ControlFlow::Break(result) = self.advance(out, None) {
            return Poll::Ready(result.map_err(Failure::Deflate));
        }
        loop {
            let (flow, ready) = match source.fill(cx) {
                Poll::Pending => (self.advance(out, None), false),
                Poll::Ready(Ok(())) => {
                    let mut taken = 0;
                    let flow = self.advance(out, Some((&source.chunk[..], &mut taken)));
                    source.chunk.advance(taken.min(source.chunk.len()));
                    (flow, true)
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(Failure::Body(error))),
            };
            if let ControlFlow::Break(result) = flow {
                return Poll::Ready(result.map_err(Failure::Deflate));
            }
            if !ready {
                return if out.len == 0 {
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(()))
                };
            }
        }
    }

    /// Moves the stream on with `input` (`None` when the source has nothing
    /// ready, empty when it has ended), counting in `taken` what it takes
    /// of it. Breaks when the caller must return; continues when it should
    /// read more.
    fn advance(
        &mut self,
        out: &mut Out,
        input: Option<(&[u8], &mut usize)>,
    ) -> ControlFlow<io::Result<()>> {
        let mut input = input;
        loop {
            self.step = match std::mem::replace(&mut self.step, Step::Done) {
                Step::Encoding { read } => match input.as_mut() {
                    // Nothing ready and nothing read: wait, or send what
                    // the frame holds.
                    None if read == 0 => {
                        self.step = Step::Encoding { read };
                        if out.len == 0 {
                            break;
                        }
                        return ControlFlow::Break(Ok(()));
                    }
                    // Nothing ready: flush what was read, so the client gets
                    // it now.
                    None => Step::Flushing,
                    Some((input, _)) if input.is_empty() => Step::Finishing,
                    Some((input, taken)) => match self.encode(input, out) {
                        Ok(n) => {
                            **taken += n;
                            self.step = Step::Encoding { read: read + n };
                            break;
                        }
                        Err(error) => Step::Failed(error),
                    },
                },
                Step::Flushing => match self.flush(out) {
                    Ok(true) => {
                        self.step = Step::Encoding { read: 0 };
                        break;
                    }
                    Ok(false) => Step::Flushing,
                    Err(error) => Step::Failed(error),
                },
                Step::Finishing => match self.finish(out) {
                    Ok(true) => Step::Done,
                    Ok(false) => Step::Finishing,
                    Err(error) => Step::Failed(error),
                },
                Step::Done => return ControlFlow::Break(Ok(())),
                // The step stays `Done`.
                Step::Failed(error) => return ControlFlow::Break(Err(error)),
            };
            if let Step::Failed(_) = self.step {
                // The frame goes out first if it holds anything.
                if out.len > 0 {
                    return ControlFlow::Break(Ok(()));
                }
                continue;
            }
            if out.is_full() {
                return ControlFlow::Break(Ok(()));
            }
        }
        if out.is_full() {
            ControlFlow::Break(Ok(()))
        } else {
            ControlFlow::Continue(())
        }
    }

    /// Writes the header if it is not out yet, then deflates `input` into
    /// the room of `out`; returns how much of `input` it took.
    fn encode(&mut self, input: &[u8], out: &mut Out) -> io::Result<usize> {
        let mut taken = 0;
        loop {
            match self.part {
                Part::Header(_) => self.write_header(out),
                Part::Deflate => {
                    self.flushed = false;
                    let rest = &input[taken..];
                    let (status, n) = out.deflate(&mut self.deflate, rest, FlushCompress::None)?;
                    match status {
                        Status::Ok => {}
                        Status::StreamEnd => {
                            return Err(io::Error::other("the deflate stream ended early"));
                        }
                        Status::BufError => return Err(io::Error::other("unexpected BufError")),
                    }
                    self.crc.update(&rest[..n]);
                    taken += n;
                }
                Part::Footer(..) | Part::Done => {
                    return Err(io::Error::other("encode after complete"));
                }
            }
            if taken == input.len() || out.is_full() {
                return Ok(taken);
            }
        }
    }

    /// Flushes the compressor into `out`: true once all it held is written.
    fn flush(&mut self, out: &mut Out) -> io::Result<bool> {
        loop {
            let done = match self.part {
                Part::Header(_) => {
                    self.write_header(out);
                    false
                }
                Part::Deflate => self.flush_deflate(out)?,
                Part::Footer(..) => self.write_footer(out),
                Part::Done => true,
            };
            if done {
                return Ok(true);
            }
            if out.is_full() {
                return Ok(false);
            }
        }
    }

    fn flush_deflate(&mut self, out: &mut Out) -> io::Result<bool> {
        if self.flushed {
            return Ok(true);
        }
        out.deflate(&mut self.deflate, &[], FlushCompress::Sync)?;
        loop {
            let before = out.len;
            out.deflate(&mut self.deflate, &[], FlushCompress::None)?;
            if out.len == before {
                break;
            }
        }
        self.flushed = !out.is_full();
        Ok(self.flushed)
    }

    /// Ends the deflate stream and writes the footer: true once all of it
    /// is in `out`.
    fn finish(&mut self, out: &mut Out) -> io::Result<bool> {
        loop {
            match self.part {
                Part::Header(_) => self.write_header(out),
                Part::Deflate => {
                    self.flushed = false;
                    let (status, _) = out.deflate(&mut self.deflate, &[], FlushCompress::Finish)?;
                    match status {
                        Status::Ok => {}
                        Status::StreamEnd => {
                            let mut footer = [0; 8];
                            footer[..4].copy_from_slice(&self.crc.sum().to_le_bytes());
                            footer[4..].copy_from_slice(&self.crc.amount().to_le_bytes());
                            self.part = Part::Footer(footer, 0);
                        }
                        Status::BufError => return Err(io::Error::other("unexpected BufError")),
                    }
                }
                Part::Footer(..) => {
                    self.write_footer(out);
                }
                Part::Done => {}
            }
            if let Part::Done = self.part {
                return Ok(true);
            }
            if out.is_full() {
                return Ok(false);
            }
        }
    }

    fn write_header(&mut self, out: &mut Out) {
        if let Part::Header(written) = &mut self.part {
            *written += out.copy(HEADER.get(*written..).unwrap_or_default());
            if *written == HEADER.len() {
                self.part = Part::Deflate;
            }
        }
    }

    /// Writes what is left of the footer: true once all of it is out.
    fn write_footer(&mut self, out: &mut Out) -> bool {
        if let Part::Footer(footer, written) = &mut self.part {
            *written += out.copy(footer.get(*written..).unwrap_or_default());
            if *written == footer.len() {
                self.part = Part::Done;
                return true;
            }
            return false;
        }
        matches!(self.part, Part::Done)
    }
}
