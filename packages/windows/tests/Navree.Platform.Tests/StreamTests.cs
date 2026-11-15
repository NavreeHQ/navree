using System.Net;
using System.Net.Http.Headers;
using System.Text;
using Navree.Design;

namespace Navree.Platform.Tests;

/// <summary>A stream of server-sent events: only each event's name is passed on, and a stream the server makes unbounded is ended.</summary>
[TestClass]
public sealed class StreamTests
{
    private static readonly Origins Shop = new("https://shop.example");

    private static HttpResponseMessage Events(string body, string type = "text/event-stream")
    {
        ByteArrayContent content = new(Encoding.UTF8.GetBytes(body));
        content.Headers.ContentType = new MediaTypeHeaderValue(type);
        return new HttpResponseMessage(HttpStatusCode.OK) { Content = content };
    }

    [TestMethod]
    public async Task A_stream_gives_the_name_of_each_event_and_nothing_it_carried()
    {
        Answering handler = new(_ => Events(": a comment\nevent: orders\ndata: {\"secret\":1}\n\ndata: x\r\n\r\nevent: stock\n\n"));
        using HttpFetcher fetcher = new(Shop, new CookieStore(Shop), handler);
        List<string?> names = [];
        bool opened = false;

        int status = await fetcher.StreamAsync(
            "https://shop.example/_navree/live",
            [new("x-navree-native", "1")],
            () =>
            {
                opened = true;
                return Task.CompletedTask;
            },
            name =>
            {
                names.Add(name);
                return Task.CompletedTask;
            },
            CancellationToken.None);

        Assert.AreEqual(200, status);
        Assert.IsTrue(opened);
        CollectionAssert.AreEqual(new string?[] { "orders", null, "stock" }, names);
        Assert.AreEqual("1", handler.Asked.Single().Headers.GetValues("x-navree-native").Single());
    }

    [TestMethod]
    public async Task An_address_that_is_not_the_apps_is_never_streamed_from()
    {
        Answering handler = new(_ => Events(string.Empty));
        using HttpFetcher fetcher = new(Shop, new CookieStore(Shop), handler);
        int status = await fetcher.StreamAsync("https://evil.example/live", [], () => Task.CompletedTask, _ => Task.CompletedTask, CancellationToken.None);

        Assert.AreEqual(0, status);
        Expect.That(handler.Asked.Count == 0, "nothing was asked");
    }

    [TestMethod]
    public async Task What_is_not_an_event_stream_is_answered_by_its_status_and_never_opened()
    {
        using HttpFetcher notFound = new(Shop, new CookieStore(Shop), new Answering(_ => new HttpResponseMessage(HttpStatusCode.NotFound)));
        bool opened = false;
        Task Open()
        {
            opened = true;
            return Task.CompletedTask;
        }

        Assert.AreEqual(404, await notFound.StreamAsync("https://shop.example/live", [], Open, _ => Task.CompletedTask, CancellationToken.None));
        using HttpFetcher page = new(Shop, new CookieStore(Shop), new Answering(_ => Events("hello", "text/html")));
        Assert.AreEqual(200, await page.StreamAsync("https://shop.example/live", [], Open, _ => Task.CompletedTask, CancellationToken.None));
        Assert.IsFalse(opened);
    }

    [TestMethod]
    public async Task A_line_that_never_ends_ends_the_stream_and_no_answer_is_zero()
    {
        string endless = "data: " + new string('a', EventStream.MostLineBytes + 10);
        using HttpFetcher long_ = new(Shop, new CookieStore(Shop), new Answering(_ => Events(endless)));
        Assert.AreEqual(0, await long_.StreamAsync("https://shop.example/live", [], () => Task.CompletedTask, _ => Task.CompletedTask, CancellationToken.None));

        using HttpFetcher down = new(Shop, new CookieStore(Shop), new Answering(_ => throw new HttpRequestException("no route")));
        Assert.AreEqual(0, await down.StreamAsync("https://shop.example/live", [], () => Task.CompletedTask, _ => Task.CompletedTask, CancellationToken.None));
    }
}
