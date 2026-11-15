namespace Navree.Core;

/// <summary>
/// The runtime core: everything a client does besides drawing, placing and
/// carrying out effects. JSON in, JSON out, as UTF-8 bytes.
/// </summary>
/// <remarks>
/// Call it from one thread, in the order things happened. Every call and
/// <see cref="Dispose"/> hold the same lock, so a call made while another
/// thread ends the core either finishes first or finds the core ended: the
/// native pointer is never used after it was given back.
/// </remarks>
/// <example>
/// <code>
/// using NavreeCore core = new("""{"origin":"https://shop.example","client":{"platform":"windows","form":"desktop","vocab":1,"text_percent":100}}"""u8);
/// byte[] update = core.Send("""{"t":"start"}"""u8);
/// byte[] nav = core.View("""{"part":"nav"}"""u8);
/// </code>
/// </example>
public sealed class NavreeCore : IDisposable
{
    private readonly Lock gate = new();
    private readonly CoreHandle handle;

    /// <summary>Makes a core from a configuration, as JSON.</summary>
    /// <param name="config">The configuration: the app's origin and its client descriptor, and the rest the core's README lists.</param>
    /// <exception cref="ArgumentException">The configuration does not read.</exception>
    public NavreeCore(ReadOnlySpan<byte> config)
    {
        handle = CoreNative.Create(config);
    }

    /// <summary>Tells the core one thing that happened; answers what changed and what to do.</summary>
    /// <param name="eventJson">One event, such as <c>{"t":"start"}</c>.</param>
    /// <returns>The update, or <c>{"error":"..."}</c>.</returns>
    /// <exception cref="ObjectDisposedException">The core has ended.</exception>
    public byte[] Send(ReadOnlySpan<byte> eventJson)
    {
        lock (gate)
        {
            ObjectDisposedException.ThrowIf(handle.IsClosed, this);
            return CoreNative.Send(handle, eventJson);
        }
    }

    /// <summary>Reads a part of the view state; an empty query reads all of it.</summary>
    /// <param name="query">A query such as <c>{"part":"nav"}</c>.</param>
    /// <returns>That part, or <c>{"error":"..."}</c>.</returns>
    /// <exception cref="ObjectDisposedException">The core has ended.</exception>
    public byte[] View(ReadOnlySpan<byte> query = default)
    {
        lock (gate)
        {
            ObjectDisposedException.ThrowIf(handle.IsClosed, this);
            return CoreNative.View(handle, query);
        }
    }

    /// <summary>Ends the core. Calling it again does nothing.</summary>
    public void Dispose()
    {
        lock (gate)
        {
            handle.Dispose();
        }
    }
}
