using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Text;
using Navree.Design;
using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>A handler that fails as a transport does.</summary>
internal sealed class Failing(Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> answer) : HttpMessageHandler
{
    internal int Asked { get; private set; }

    protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        Asked++;
        return answer(request, cancellationToken);
    }
}

/// <summary>
/// A request is reported as never sent (<c>offline</c>) only when nothing of it was sent: the name was not found, or no connection was made. A
/// request that was sent and whose answer was lost is <c>timeout</c> or <c>other</c>, and so is one when it cannot be told.
/// </summary>
[TestClass]
public sealed class TransportTests
{
    private static readonly Origins Shop = new("https://shop.example");

    private static FetchEffect Post(string url, int timeoutMs = 5000) =>
        new(1, "post", url, [new KeyValuePair<string, string>("content-type", "application/x-www-form-urlencoded")], new FormBody("item=steel-tiffin&qty=2"), timeoutMs, false);

    private static async Task<string?> ErrorOf(Func<HttpRequestMessage, CancellationToken, Task<HttpResponseMessage>> answer, int timeoutMs = 5000)
    {
        using HttpFetcher fetcher = new(Shop, new CookieStore(Shop), new Failing(answer));
        return (await fetcher.FetchAsync(Post("https://shop.example/orders", timeoutMs), CancellationToken.None)).Error;
    }

    [TestMethod]
    public async Task A_name_that_is_not_found_is_offline()
    {
        Assert.AreEqual("offline", await ErrorOf((_, _) => throw new HttpRequestException(HttpRequestError.NameResolutionError, "No such host is known.", new SocketException((int)SocketError.HostNotFound))));
    }

    [TestMethod]
    public async Task A_connection_refused_is_offline()
    {
        Assert.AreEqual("offline", await ErrorOf((_, _) => throw new HttpRequestException(HttpRequestError.ConnectionError, "No connection could be made.", new SocketException((int)SocketError.ConnectionRefused))));
    }

    [TestMethod]
    public async Task A_connection_reset_after_the_body_was_sent_is_not_offline()
    {
        string? error = await ErrorOf(async (request, cancel) =>
        {
            _ = await request.Content!.ReadAsStringAsync(cancel);
            throw new HttpRequestException(HttpRequestError.Unknown, "An error occurred while sending the request.", new IOException("Unable to read data from the transport connection.", new SocketException((int)SocketError.ConnectionReset)));
        });
        Assert.AreEqual("other", error);
    }

    [TestMethod]
    public async Task An_answer_that_ends_early_or_a_stream_that_breaks_is_not_offline()
    {
        Assert.AreEqual("other", await ErrorOf((_, _) => throw new HttpRequestException(HttpRequestError.ResponseEnded, "The response ended prematurely.")));
        Assert.AreEqual("other", await ErrorOf((_, _) => throw new IOException("The connection was closed.", new SocketException((int)SocketError.ConnectionAborted))));
        Assert.AreEqual("other", await ErrorOf((_, _) => throw new HttpRequestException("An error occurred.")));
    }

    [TestMethod]
    public async Task A_timeout_waiting_for_the_answer_is_timeout()
    {
        Assert.AreEqual("timeout", await ErrorOf(
            async (request, cancel) =>
            {
                _ = await request.Content!.ReadAsStringAsync(cancel);
                await Task.Delay(TimeSpan.FromSeconds(30), cancel);
                return new HttpResponseMessage(HttpStatusCode.OK);
            },
            timeoutMs: 1000));
    }

    [TestMethod]
    public async Task A_redirect_is_never_followed_so_no_second_request_can_fail_and_the_answer_is_the_first_addresss()
    {
        Failing handler = new((request, _) =>
        {
            if (request.RequestUri!.AbsolutePath == "/orders")
            {
                HttpResponseMessage redirect = new(HttpStatusCode.SeeOther);
                redirect.Headers.Location = new Uri("https://shop.example/orders/1", UriKind.Absolute);
                return Task.FromResult(redirect);
            }

            throw new HttpRequestException(HttpRequestError.ConnectionError, "No connection could be made.");
        });
        using HttpFetcher fetcher = new(Shop, new CookieStore(Shop), handler);
        FetchResult result = await fetcher.FetchAsync(Post("https://shop.example/orders"), CancellationToken.None);

        Assert.AreEqual(303, result.Status);
        Assert.IsNull(result.Error);
        Assert.AreEqual("https://shop.example/orders/1", result.Location);
        Assert.AreEqual(1, handler.Asked, "the redirect was followed");
    }

    [TestMethod]
    public async Task A_real_connection_refused_on_the_loopback_is_offline()
    {
        TcpListener closed = new(IPAddress.Loopback, 0);
        closed.Start();
        int port = ((IPEndPoint)closed.LocalEndpoint).Port;
        closed.Stop();

        Origins local = new("http://127.0.0.1:" + port, loopbackHttp: true);
        using HttpFetcher fetcher = new(local, new CookieStore(local));
        FetchResult result = await fetcher.FetchAsync(Post("http://127.0.0.1:" + port + "/orders"), CancellationToken.None);
        Assert.AreEqual("offline", result.Error);
    }

    [TestMethod]
    public async Task A_real_connection_reset_after_the_request_was_read_is_not_offline()
    {
        using TcpListener server = new(IPAddress.Loopback, 0);
        server.Start();
        int port = ((IPEndPoint)server.LocalEndpoint).Port;
        Task serving = Task.Run(async () =>
        {
            using Socket accepted = await server.AcceptSocketAsync();
            byte[] buffer = new byte[64 * 1024];
            StringBuilder seen = new();
            while (!seen.ToString().Contains("item=steel-tiffin", StringComparison.Ordinal))
            {
                int read = await accepted.ReceiveAsync(buffer, SocketFlags.None);
                if (read == 0)
                {
                    break;
                }

                seen.Append(Encoding.ASCII.GetString(buffer, 0, read));
            }

            // The whole request, body and all, has arrived: the connection is reset with no answer.
            accepted.LingerState = new LingerOption(true, 0);
            accepted.Close();
        });

        Origins local = new("http://127.0.0.1:" + port, loopbackHttp: true);
        using HttpFetcher fetcher = new(local, new CookieStore(local));
        FetchResult result = await fetcher.FetchAsync(Post("http://127.0.0.1:" + port + "/orders"), CancellationToken.None);
        await serving.WaitAsync(TimeSpan.FromSeconds(5));
        Assert.AreNotEqual("offline", result.Error, "a request whose body reached the server was reported as never sent");
        Assert.IsNotNull(result.Error);
    }
}
