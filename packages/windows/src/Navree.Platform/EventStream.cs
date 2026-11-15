using System.Text;

namespace Navree.Platform;

/// <summary>
/// Reads a stream of server-sent events. The core asks only for an event's name: when an event ends, the name it
/// was given (or null for the default one) is passed on, and what the event carried is not. A line longer than
/// <see cref="MostLineBytes"/> ends the stream: what a server sends is bounded, however it behaves.
/// </summary>
public static class EventStream
{
    /// <summary>The most bytes one line of a stream may have: 256 KiB.</summary>
    public const int MostLineBytes = 256 * 1024;

    /// <summary>Reads events until the stream ends.</summary>
    /// <param name="stream">The bytes of the response.</param>
    /// <param name="onEvent">Called when an event ends, with its name or null.</param>
    /// <param name="cancel">Cancelled when the stream is no longer wanted.</param>
    /// <exception cref="IOException">A line was longer than the limit.</exception>
    public static async Task ReadAsync(Stream stream, Func<string?, Task> onEvent, CancellationToken cancel)
    {
        ArgumentNullException.ThrowIfNull(stream);
        ArgumentNullException.ThrowIfNull(onEvent);
        using StreamReader reader = new(new LimitedLines(stream), new UTF8Encoding(false), detectEncodingFromByteOrderMarks: false, bufferSize: 8192);
        string? name = null;
        bool any = false;
        while (await reader.ReadLineAsync(cancel).ConfigureAwait(false) is string line)
        {
            if (line.Length == 0)
            {
                if (any)
                {
                    await onEvent(name).ConfigureAwait(false);
                }

                name = null;
                any = false;
                continue;
            }

            if (line[0] == ':')
            {
                continue;
            }

            int colon = line.IndexOf(':', StringComparison.Ordinal);
            string field = colon < 0 ? line : line[..colon];
            string value = colon < 0 ? string.Empty : line[(colon + 1)..];
            if (value.StartsWith(' '))
            {
                value = value[1..];
            }

            switch (field)
            {
                case "event":
                    name = value.Length == 0 ? null : value;
                    any = true;
                    break;
                case "data":
                    any = true;
                    break;
                default:
                    break;
            }
        }
    }

    // A stream that gives up when one line has run on past the limit.
    private sealed class LimitedLines(Stream inner) : Stream
    {
        private int run;

        public override bool CanRead => true;

        public override bool CanSeek => false;

        public override bool CanWrite => false;

        public override long Length => throw new NotSupportedException();

        public override long Position
        {
            get => throw new NotSupportedException();
            set => throw new NotSupportedException();
        }

        public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default)
        {
            int read = await inner.ReadAsync(buffer, cancellationToken).ConfigureAwait(false);
            Count(buffer.Span[..read]);
            return read;
        }

        public override int Read(byte[] buffer, int offset, int count)
        {
            int read = inner.Read(buffer, offset, count);
            Count(buffer.AsSpan(offset, read));
            return read;
        }

        public override void Flush()
        {
        }

        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();

        public override void SetLength(long value) => throw new NotSupportedException();

        public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();

        protected override void Dispose(bool disposing)
        {
            if (disposing)
            {
                inner.Dispose();
            }

            base.Dispose(disposing);
        }

        private void Count(ReadOnlySpan<byte> bytes)
        {
            foreach (byte next in bytes)
            {
                run = next is (byte)'\n' or (byte)'\r' ? 0 : run + 1;
                if (run > MostLineBytes)
                {
                    throw new IOException("a line of the stream is longer than the limit");
                }
            }
        }
    }
}
