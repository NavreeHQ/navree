using Navree.Design;

namespace Navree.Platform.Tests;

/// <summary>The rules that keep a request, a cookie and a header to what the app may do.</summary>
[TestClass]
public sealed class RuleTests
{
    private static readonly DateTimeOffset Now = new(2026, 11, 15, 12, 0, 0, TimeSpan.Zero);

    [TestMethod]
    public void A_header_that_HTTP_cannot_carry_is_refused()
    {
        Assert.IsTrue(HeaderRules.CanCarry("x-navree-client", "platform=windows; form=desktop; vocab=0"));
        Assert.IsTrue(HeaderRules.CanCarry("accept", "application/vnd.navree.native+json"));
        Assert.IsFalse(HeaderRules.CanCarry("x-a", "line\r\nbreak: injected"));
        Assert.IsFalse(HeaderRules.CanCarry("x-a", "a\nb"));
        Assert.IsFalse(HeaderRules.CanCarry("x-a", "café"));
        Assert.IsFalse(HeaderRules.CanCarry("x-a", "nul\0"));
        Assert.IsFalse(HeaderRules.CanCarry("bad name", "v"));
        Assert.IsFalse(HeaderRules.CanCarry(string.Empty, "v"));
        Assert.IsFalse(HeaderRules.CanCarry("x-a", new string('a', HeaderRules.MostValueLength + 1)));
    }

    [TestMethod]
    public void Only_the_navree_headers_and_cache_control_are_handed_to_the_core()
    {
        Dictionary<string, IEnumerable<string>> headers = new()
        {
            ["X-Navree-Keep"] = ["1"],
            ["Cache-Control"] = ["max-age=60", "private"],
            ["Set-Cookie"] = ["session=secret"],
            ["Content-Type"] = ["application/json"],
        };
        KeyValuePair<string, string>[] passed = [.. HeaderRules.Passed(headers)];
        CollectionAssert.AreEqual(
            new[] { "x-navree-keep=1", "cache-control=max-age=60", "cache-control=private" },
            passed.Select(header => header.Key + "=" + header.Value).ToArray());
    }

    [TestMethod]
    public void Cookies_are_kept_only_from_the_apps_origins_and_sent_only_to_them()
    {
        CookieStore store = new(new Origins("https://shop.example", ["https://*.shop.example"]));
        store.Store("https://shop.example/login", ["session=abc; Path=/; Secure; HttpOnly"], Now);
        store.Store("https://evil.example/login", ["stolen=1"], Now);
        store.Store("http://shop.example/login", ["plain=1"], Now);

        Expect.That(store.Count == 1, "only the cookie of an origin of the app is kept");
        Assert.AreEqual("session=abc", store.Header("https://shop.example/orders", Now));
        Assert.IsNull(store.Header("https://evil.example/orders", Now));
        Assert.IsNull(store.Header("https://shop.example.evil.example/orders", Now));
    }

    [TestMethod]
    public void A_cookie_is_for_its_host_unless_it_names_a_domain_its_host_is_under()
    {
        CookieStore store = new(new Origins("https://shop.example", ["https://*.shop.example"]));
        store.Store("https://shop.example/", ["hostonly=1", "wide=2; Domain=shop.example"], Now);
        store.Store("https://a.shop.example/", ["foreign=3; Domain=evil.example", "other=4; Domain=b.shop.example"], Now);

        Assert.AreEqual("hostonly=1; wide=2", store.Header("https://shop.example/x", Now));
        Assert.AreEqual("wide=2", store.Header("https://a.shop.example/x", Now));
        Expect.That(store.Count == 2, "a cookie for a domain its host is not under is refused");
    }

    [TestMethod]
    public void A_cookie_that_has_expired_is_gone_and_one_that_is_set_to_expire_removes_the_old()
    {
        CookieStore store = new(new Origins("https://shop.example"));
        store.Store("https://shop.example/", ["a=1; Max-Age=60", "b=2"], Now);
        Assert.AreEqual("a=1; b=2", store.Header("https://shop.example/", Now));
        Assert.AreEqual("b=2", store.Header("https://shop.example/", Now.AddSeconds(61)));
        store.Store("https://shop.example/", ["b=; Max-Age=0"], Now);
        Assert.IsNull(store.Header("https://shop.example/", Now.AddSeconds(61)));
    }

    [TestMethod]
    public void The_xsrf_token_is_the_cookie_decoded()
    {
        CookieStore store = new(new Origins("https://shop.example"));
        store.Store("https://shop.example/", ["XSRF-TOKEN=a%3Db%2Fc; Path=/"], Now);
        Assert.AreEqual("a=b/c", store.XsrfToken("https://shop.example/x", Now));
        Assert.IsNull(store.XsrfToken("https://evil.example/x", Now));
    }

    [TestMethod]
    public void Cookies_written_down_read_back_and_a_store_can_be_emptied()
    {
        CookieStore store = new(new Origins("https://shop.example"));
        store.Store("https://shop.example/", ["a=1; Path=/x; Secure; Max-Age=600", "b=2"], Now);
        string written = store.Export();

        CookieStore other = new(new Origins("https://shop.example"));
        other.Import(written);
        Assert.AreEqual(store.Header("https://shop.example/x/y", Now), other.Header("https://shop.example/x/y", Now));
        other.Import("not json");
        Expect.That(store.Count == other.Count, "what does not read changes nothing");

        store.Clear();
        Expect.That(store.Count == 0, "a store can be emptied");
    }

    [TestMethod]
    public async Task A_wipe_runs_in_order_and_a_step_that_fails_or_hangs_does_not_stop_the_rest()
    {
        List<string> order = [];
        WipeReport report = await Wipe.RunAsync(
            [
                new WipeStep("cache", () => { order.Add("cache"); return Task.CompletedTask; }),
                new WipeStep("hangs", () => Task.Delay(TimeSpan.FromSeconds(30))),
                new WipeStep("fails", () => throw new InvalidOperationException("no")),
                new WipeStep("cookies", () => { order.Add("cookies"); return Task.CompletedTask; }),
            ],
            TimeSpan.FromMilliseconds(300));

        CollectionAssert.AreEqual(new[] { "cache", "cookies" }, order);
        CollectionAssert.AreEqual(new[] { "cache", "cookies" }, report.Done.ToArray());
        CollectionAssert.AreEqual(new[] { "hangs", "fails" }, report.Failed.Select(failure => failure.Key).ToArray());
        Assert.IsFalse(report.Complete);
    }

    [TestMethod]
    public async Task A_wipe_step_whose_state_lives_on_one_thread_runs_there_and_the_wipe_waits_for_it()
    {
        // One thread that owns its state, as the window's thread does, and runs what is handed to it in turn.
        using System.Collections.Concurrent.BlockingCollection<Action> handed = [];
        Thread owner = new(() =>
        {
            foreach (Action action in handed.GetConsumingEnumerable())
            {
                action();
            }
        }) { IsBackground = true };
        owner.Start();
        Task OnOwner(Func<Task> step)
        {
            TaskCompletionSource done = new(TaskCreationOptions.RunContinuationsAsynchronously);
            handed.Add(() =>
            {
                Task ran;
                try
                {
                    ran = step();
                }
                catch (Exception problem)
                {
                    ran = Task.FromException(problem);
                }

                _ = ran.ContinueWith(finished => _ = finished.IsFaulted ? done.TrySetException(finished.Exception!.InnerExceptions) : done.TrySetResult(), TaskScheduler.Default);
            });
            return done.Task;
        }

        int ranOn = 0;
        List<string> order = [];
        WipeReport report = await Wipe.RunAsync(
            [
                new WipeStep("the pictures", () =>
                {
                    ranOn = Environment.CurrentManagedThreadId;
                    order.Add("the pictures");
                    return Task.CompletedTask;
                }, OnOwner),
                new WipeStep("fails where it lives", () => throw new InvalidOperationException("no"), OnOwner),
                new WipeStep("after", () => { order.Add("after"); return Task.CompletedTask; }),
            ],
            TimeSpan.FromSeconds(5));
        handed.CompleteAdding();

        Assert.AreEqual(owner.ManagedThreadId, ranOn, "the step ran on another thread than the one its state lives on");
        CollectionAssert.AreEqual(new[] { "the pictures", "after" }, order.ToArray());
        CollectionAssert.AreEqual(new[] { "fails where it lives" }, report.Failed.Select(failure => failure.Key).ToArray());
    }
}
