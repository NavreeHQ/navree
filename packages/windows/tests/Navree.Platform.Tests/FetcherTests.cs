using System.Net;
using System.Net.Http.Headers;
using System.Text;
using Navree.Design;
using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>A handler that answers from a function and remembers what it was asked.</summary>
internal sealed class Answering(Func<HttpRequestMessage, HttpResponseMessage> answer) : HttpMessageHandler
{
    internal List<HttpRequestMessage> Asked { get; } = [];

    internal List<string> Bodies { get; } = [];

    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        Asked.Add(request);
        Bodies.Add(request.Content is null ? string.Empty : await request.Content.ReadAsStringAsync(cancellationToken));
        return answer(request);
    }
}

/// <summary>What the client asks for and what it hands back, held to the rules the core's README states.</summary>
[TestClass]
public sealed class FetcherTests
{
    private static readonly Origins Shop = new("https://shop.example");

    private static FetchEffect Get(string url, bool xsrf = false, RequestBody? body = null, string method = "get", params KeyValuePair<string, string>[] headers) =>
        new(1, method, url, headers, body, 5000, xsrf);

    [TestMethod]
    public async Task A_request_to_an_address_that_is_not_the_apps_is_never_made()
    {
        Answering handler = new(_ => new HttpResponseMessage(HttpStatusCode.OK));
        using HttpFetcher fetcher = new(Shop, new CookieStore(Shop), handler);
        foreach (string url in new[] { "https://evil.example/x", "http://shop.example/x", "https://shop.example@evil.example/", "https://shop.example/a b", "/relative" })
        {
            FetchResult result = await fetcher.FetchAsync(Get(url), CancellationToken.None);
            Assert.AreEqual("other", result.Error, url);
        }

        Expect.That(handler.Asked.Count == 0, "nothing was asked");
    }

    [TestMethod]
    public async Task A_redirect_is_handed_back_as_it_came_and_never_followed()
    {
        Answering handler = new(_ =>
        {
            HttpResponseMessage redirect = new(HttpStatusCode.Found);
            redirect.Headers.Location = new Uri("https://evil.example/stolen", UriKind.Absolute);
            redirect.Headers.TryAddWithoutValidation("Set-Cookie", "after=1; Path=/");
            return redirect;
        });
        CookieStore cookies = new(Shop);
        using HttpFetcher fetcher = new(Shop, cookies, handler);
        FetchResult result = await fetcher.FetchAsync(Get("https://shop.example/go"), CancellationToken.None);

        Assert.AreEqual(302, result.Status);
        Assert.AreEqual("https://evil.example/stolen", result.Location);
        Expect.That(handler.Asked.Count == 1, "the redirect was not followed");
        Assert.AreEqual("after=1", cookies.Header("https://shop.example/", DateTimeOffset.UtcNow));
    }

    [TestMethod]
    public void The_real_connections_follow_no_redirect_and_keep_no_cookies_of_their_own()
    {
        using SocketsHttpHandler connections = HttpFetcher.Connections();
        Assert.IsFalse(connections.AllowAutoRedirect);
        Assert.IsFalse(connections.UseCookies);
    }

    [TestMethod]
    public async Task A_request_carries_the_headers_the_core_gave_the_cookies_of_the_app_and_the_token()
    {
        Answering handler = new(_ => new HttpResponseMessage(HttpStatusCode.OK));
        CookieStore cookies = new(Shop);
        cookies.Store("https://shop.example/", ["session=abc; Path=/", "XSRF-TOKEN=t%2Fok; Path=/"], DateTimeOffset.UtcNow);
        using HttpFetcher fetcher = new(Shop, cookies, handler);

        await fetcher.FetchAsync(
            Get(
                "https://shop.example/orders",
                xsrf: true,
                body: new FormBody("a=1&b=2"),
                method: "post",
                headers: [new("x-navree-native", "1"), new("content-type", "application/x-www-form-urlencoded"), new("cookie", "smuggled=1")]),
            CancellationToken.None);

        HttpRequestMessage sent = handler.Asked.Single();
        Assert.AreEqual("1", sent.Headers.GetValues("x-navree-native").Single());
        Assert.AreEqual("session=abc; XSRF-TOKEN=t%2Fok", sent.Headers.GetValues("Cookie").Single());
        Assert.AreEqual("t/ok", sent.Headers.GetValues("x-xsrf-token").Single());
        Assert.AreEqual("a=1&b=2", handler.Bodies.Single());
        Assert.AreEqual("application/x-www-form-urlencoded", sent.Content!.Headers.ContentType!.ToString());
        Assert.AreEqual(HttpMethod.Post, sent.Method);
    }

    [TestMethod]
    public async Task A_header_that_cannot_be_carried_makes_the_request_a_failed_one_and_sends_nothing()
    {
        Answering handler = new(_ => new HttpResponseMessage(HttpStatusCode.OK));
        using HttpFetcher fetcher = new(Shop, new CookieStore(Shop), handler);
        FetchResult result = await fetcher.FetchAsync(Get("https://shop.example/", headers: [new("x-a", "one\r\ntwo")]), CancellationToken.None);

        Assert.AreEqual("other", result.Error);
        Expect.That(handler.Asked.Count == 0, "nothing was asked");
    }

    [TestMethod]
    public async Task Only_the_navree_headers_and_cache_control_come_back_and_the_cookie_of_the_answer_does_not()
    {
        Answering handler = new(_ =>
        {
            ByteArrayContent content = new(Encoding.UTF8.GetBytes("{\"navree\":1}"));
            content.Headers.ContentType = new("application/vnd.navree.native+json");
            HttpResponseMessage answer = new(HttpStatusCode.OK) { Content = content };
            answer.Headers.TryAddWithoutValidation("X-Navree-Keep", "1");
            answer.Headers.TryAddWithoutValidation("Cache-Control", "no-store");
            answer.Headers.TryAddWithoutValidation("Set-Cookie", "session=abc");
            answer.Headers.TryAddWithoutValidation("Server", "x");
            return answer;
        });
        using HttpFetcher fetcher = new(Shop, new CookieStore(Shop), handler);
        FetchResult result = await fetcher.FetchAsync(Get("https://shop.example/"), CancellationToken.None);

        Assert.AreEqual(200, result.Status);
        Assert.AreEqual("{\"navree\":1}", result.Body);
        Assert.AreEqual("application/vnd.navree.native+json", result.ContentType);
        CollectionAssert.AreEquivalent(new[] { "x-navree-keep=1", "cache-control=no-store" }, result.Headers.Select(header => header.Key + "=" + header.Value).ToArray());
        Assert.IsNull(result.Location);
    }

    [TestMethod]
    public async Task A_body_over_the_limit_is_a_failed_request()
    {
        Answering handler = new(_ => new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent(new byte[(int)HttpFetcher.MostBodyBytes + 1]) });
        using HttpFetcher fetcher = new(Shop, new CookieStore(Shop), handler);
        FetchResult result = await fetcher.FetchAsync(Get("https://shop.example/"), CancellationToken.None);
        Assert.AreEqual("other", result.Error);
    }

    [TestMethod]
    public async Task No_answer_is_offline_or_timeout_and_a_cancelled_request_is_other()
    {
        Answering offline = new(_ => throw new HttpRequestException(HttpRequestError.ConnectionError, "no route", new System.Net.Sockets.SocketException((int)System.Net.Sockets.SocketError.NetworkUnreachable)));
        using HttpFetcher down = new(Shop, new CookieStore(Shop), offline);
        Assert.AreEqual("offline", (await down.FetchAsync(Get("https://shop.example/"), CancellationToken.None)).Error);

        using CancellationTokenSource gone = new();
        Answering slow = new(_ => { gone.Cancel(); throw new OperationCanceledException(gone.Token); });
        using HttpFetcher cancelled = new(Shop, new CookieStore(Shop), slow);
        Assert.AreEqual("other", (await cancelled.FetchAsync(Get("https://shop.example/"), gone.Token)).Error);
    }
}
