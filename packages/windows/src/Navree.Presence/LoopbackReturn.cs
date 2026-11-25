using System.Net;
using System.Net.Sockets;
using System.Text;

namespace Navree.Presence;

/// <summary>
/// A sign-in's return on a port of this machine: the browser is sent to <c>http://127.0.0.1:port/...</c>, where the app is
/// listening for that one request. It listens on the loopback address only (so nothing off the machine can reach it), takes
/// one request, answers it with a short page that says the person may go back to the app, and stops.
/// </summary>
/// <remarks>
/// The app starts none: the server gives a Windows app's sign-in to a link of the app's own scheme alone, so a sign-in that would come back
/// on a port is not started (<see cref="BrowserSignIn.WhyNotReturn"/>) and the app listens on no port.
/// </remarks>
public sealed class LoopbackReturn : IDisposable
{
    private const int LargestRequest = 8192;
    private readonly TcpListener listener;

    private LoopbackReturn(TcpListener listener, int port)
    {
        this.listener = listener;
        Port = port;
    }

    /// <summary>The port it listens on.</summary>
    public int Port { get; }

    /// <summary>Starts listening on the loopback address.</summary>
    /// <param name="port">The port, or 0 for any the system has free (the way a loopback return is meant to work).</param>
    /// <returns>The listener.</returns>
    public static LoopbackReturn Start(int port = 0)
    {
        TcpListener listener = new(IPAddress.Loopback, port);
        listener.Start(1);
        return new LoopbackReturn(listener, ((IPEndPoint)listener.LocalEndpoint).Port);
    }

    /// <summary>
    /// The path and query of a request line, when it is a plain <c>GET</c> of a path: <c>GET /signed-in?code=1 HTTP/1.1</c>.
    /// Anything else (another method, an absolute address, a target with a control character) is not taken.
    /// </summary>
    /// <param name="requestLine">The first line of the request.</param>
    /// <returns>The target, or null.</returns>
    public static string? Target(string requestLine)
    {
        string[] parts = requestLine.Split(' ');
        if (parts.Length != 3 || parts[0] != "GET" || !parts[2].StartsWith("HTTP/1.", StringComparison.Ordinal))
        {
            return null;
        }

        string target = parts[1];
        return OpeningRules.IsAppPath(target) ? target : null;
    }

    /// <summary>Waits for the one request and returns the whole address it was made to.</summary>
    /// <param name="cancel">Ends the wait.</param>
    /// <returns>The whole address, or null when the wait was ended or what came was not a request for a path.</returns>
    public async Task<string?> WaitAsync(CancellationToken cancel)
    {
        try
        {
            while (!cancel.IsCancellationRequested)
            {
                using TcpClient client = await listener.AcceptTcpClientAsync(cancel).ConfigureAwait(false);
                if (client.Client.RemoteEndPoint is not IPEndPoint from || !IPAddress.IsLoopback(from.Address))
                {
                    continue;
                }

                string? target = await ReadAsync(client, cancel).ConfigureAwait(false);
                if (target is not null)
                {
                    return "http://127.0.0.1:" + Port.ToString(System.Globalization.CultureInfo.InvariantCulture) + target;
                }
            }
        }
        catch (OperationCanceledException)
        {
        }
        catch (ObjectDisposedException)
        {
        }
        catch (SocketException)
        {
        }

        return null;
    }

    /// <inheritdoc />
    public void Dispose() => listener.Stop();

    private static async Task<string?> ReadAsync(TcpClient client, CancellationToken cancel)
    {
        using CancellationTokenSource limit = CancellationTokenSource.CreateLinkedTokenSource(cancel);
        limit.CancelAfter(TimeSpan.FromSeconds(10));
        NetworkStream stream = client.GetStream();
        byte[] buffer = new byte[LargestRequest];
        int held = 0;
        try
        {
            while (held < buffer.Length)
            {
                int read = await stream.ReadAsync(buffer.AsMemory(held), limit.Token).ConfigureAwait(false);
                if (read == 0)
                {
                    break;
                }

                held += read;
                if (Encoding.ASCII.GetString(buffer, 0, held).Contains("\r\n", StringComparison.Ordinal))
                {
                    break;
                }
            }

            string text = Encoding.ASCII.GetString(buffer, 0, held);
            int end = text.IndexOf("\r\n", StringComparison.Ordinal);
            string? target = end > 0 ? Target(text[..end]) : null;
            byte[] page = Encoding.UTF8.GetBytes(
                "<!doctype html><meta charset=utf-8><title>Signed in</title><p>You can close this tab and go back to the app.</p>");
            string head = (target is null ? "HTTP/1.1 400 Bad Request" : "HTTP/1.1 200 OK")
                + "\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: "
                + page.Length.ToString(System.Globalization.CultureInfo.InvariantCulture)
                + "\r\n\r\n";
            await stream.WriteAsync(Encoding.ASCII.GetBytes(head), limit.Token).ConfigureAwait(false);
            if (target is not null)
            {
                await stream.WriteAsync(page, limit.Token).ConfigureAwait(false);
            }

            await stream.FlushAsync(limit.Token).ConfigureAwait(false);
            return target;
        }
        catch (OperationCanceledException)
        {
            return null;
        }
        catch (IOException)
        {
            return null;
        }
    }
}
