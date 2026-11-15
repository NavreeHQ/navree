using System.Net;
using System.Net.Sockets;

namespace Navree.Platform;

/// <summary>
/// How a connection is made to a name that has more than one address. A server that listens on one address of a name and not on the other (the
/// name <c>localhost</c> is both <c>::1</c> and <c>127.0.0.1</c>, and a server in a virtual machine may answer on only one) must not cost the
/// whole time out: the next address is tried after a short wait while the first is still trying, and the first to answer is kept.
/// </summary>
public static class Connecting
{
    /// <summary>How long the first address is given before the next is tried beside it.</summary>
    public static readonly TimeSpan Stagger = TimeSpan.FromMilliseconds(250);

    /// <summary>
    /// Tries the addresses one after the other, each starting <paramref name="stagger"/> after the one before (at once when the one before has
    /// failed), and keeps the first that connects; the others are cancelled, and any that connects after that is discarded.
    /// </summary>
    /// <typeparam name="T">What a connection is.</typeparam>
    /// <param name="addresses">The name's addresses, in the order to try them.</param>
    /// <param name="stagger">The wait before the next address is tried.</param>
    /// <param name="attempt">Connects to one address until it is cancelled.</param>
    /// <param name="discard">Lets go of a connection that came too late.</param>
    /// <param name="cancel">Cancelled when the whole connection is given up.</param>
    /// <returns>The first connection made.</returns>
    public static async Task<T> FirstAsync<T>(
        IReadOnlyList<IPAddress> addresses,
        TimeSpan stagger,
        Func<IPAddress, CancellationToken, Task<T>> attempt,
        Action<T> discard,
        CancellationToken cancel)
        where T : class
    {
        ArgumentNullException.ThrowIfNull(addresses);
        ArgumentNullException.ThrowIfNull(attempt);
        ArgumentNullException.ThrowIfNull(discard);
        if (addresses.Count == 0)
        {
            throw new SocketException((int)SocketError.HostNotFound);
        }

        using CancellationTokenSource stop = CancellationTokenSource.CreateLinkedTokenSource(cancel);
        List<Task<T>> running = [];
        int next = 0;
        Exception? last = null;
        while (true)
        {
            cancel.ThrowIfCancellationRequested();
            if (next < addresses.Count)
            {
                running.Add(attempt(addresses[next++], stop.Token));
            }

            if (running.Count == 0)
            {
                throw last ?? new SocketException((int)SocketError.HostUnreachable);
            }

            Task wait = Task.Delay(next < addresses.Count ? stagger : Timeout.InfiniteTimeSpan, stop.Token);
            Task[] all = [.. running, wait];
            Task finished = await Task.WhenAny(all).ConfigureAwait(false);
            if (ReferenceEquals(finished, wait))
            {
                // The first address has had its time: the next is tried beside it.
                continue;
            }

            Task<T> done = (Task<T>)finished;
            running.Remove(done);
            if (done.IsCompletedSuccessfully)
            {
                await stop.CancelAsync().ConfigureAwait(false);
                foreach (Task<T> late in running)
                {
                    _ = late.ContinueWith(
                        task =>
                        {
                            if (task.IsCompletedSuccessfully)
                            {
                                discard(task.Result);
                            }
                        },
                        TaskScheduler.Default);
                }

                return done.Result;
            }

            last = done.Exception?.GetBaseException();
        }
    }

    /// <summary>The connection callback the client's handler uses: the name's addresses are looked up and tried as <see cref="FirstAsync{T}"/> says.</summary>
    /// <param name="context">What the handler wants a connection to.</param>
    /// <param name="cancel">Cancelled when the handler gives up.</param>
    /// <returns>The stream of the connection, which the handler lays the secure connection over.</returns>
    public static async ValueTask<Stream> OpenAsync(System.Net.Http.SocketsHttpConnectionContext context, CancellationToken cancel)
    {
        DnsEndPoint target = context.DnsEndPoint;
        IPAddress[] addresses = await Dns.GetHostAddressesAsync(target.Host, target.AddressFamily, cancel).ConfigureAwait(false);
        Socket socket = await FirstAsync(addresses, Stagger, (address, token) => ConnectOneAsync(address, target.Port, token), static found => found.Dispose(), cancel).ConfigureAwait(false);
        return new NetworkStream(socket, ownsSocket: true);
    }

    private static async Task<Socket> ConnectOneAsync(IPAddress address, int port, CancellationToken cancel)
    {
        Socket socket = new(address.AddressFamily, SocketType.Stream, ProtocolType.Tcp) { NoDelay = true };
        try
        {
            await socket.ConnectAsync(new IPEndPoint(address, port), cancel).ConfigureAwait(false);
            return socket;
        }
        catch
        {
            socket.Dispose();
            throw;
        }
    }
}
