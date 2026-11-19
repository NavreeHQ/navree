using System.Collections.Immutable;
using System.Text;
using System.Text.Json.Nodes;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Platform.Tests;

/// <summary>A web view that is only a record of what was asked of it.</summary>
internal sealed class FakeWebView(string name, string? folder, Func<Task>? clear = null, long screen = 1) : ILiveWebView
{
    internal string Name { get; } = name;

    internal bool Emptied { get; private set; }

    internal bool AskedToClear { get; private set; }

    internal bool Closed { get; private set; }

    public long Screen { get; } = screen;

    public string? Folder { get; } = folder;

    public void Empty() => Emptied = true;

    public Task ClearProfileAsync()
    {
        AskedToClear = true;
        Expect.That(!Closed, Name + " was asked to clear its profile after it was closed");
        return clear is null ? Task.CompletedTask : clear();
    }

    public void Close() => Closed = true;
}

/// <summary>A sign-out ends every web view of the session it ends, and is done only when each is.</summary>
[TestClass]
public sealed class WebWipeTests
{
    private const string Old = @"C:\web\6";
    private const string Ended = @"C:\web\7";
    private const string Now = @"C:\web\8";
    private static readonly TimeSpan Brief = TimeSpan.FromMilliseconds(100);
    private static readonly TimeSpan Long = TimeSpan.FromSeconds(10);

    private static Task Never() => new TaskCompletionSource().Task;

    [TestMethod]
    public async Task Two_web_views_of_the_session_one_in_a_window_of_its_own_are_both_closed_and_their_profile_is_cleared_through_one()
    {
        // The page in the main window does not answer; the page in a window of its own does.
        FakeWebView main = new("the main window's page", Ended, Never);
        FakeWebView own = new("the page in a window of its own", Ended);
        int deletes = 0;

        bool done = await WebWipe.RunAsync(
            Ended,
            Now,
            [main, own],
            () =>
            {
                deletes++;
                return Task.FromResult(false);
            },
            Brief).WaitAsync(Long);

        Expect.That(done, "a profile cleared through one of the session's web views is not taken for cleared");
        Expect.That(main.Emptied && own.Emptied, "a page of the person who left was not emptied before the clear");
        Expect.That(main.Closed, "the main window's web view of the person who left is still alive");
        Expect.That(own.Closed, "the web view in a window of its own is still alive after the sign-out");
        Assert.AreEqual(0, deletes, "the folder was deleted under a profile that was cleared");
    }

    [TestMethod]
    public async Task A_web_view_left_from_an_earlier_session_never_answers_for_the_session_that_ends()
    {
        // The earlier person's web view was never closed; its profile is empty already and its clear answers at once.
        FakeWebView earlier = new("the web view of the earlier session", Old);
        FakeWebView mine = new("the web view of the session that ends", Ended, () => Task.FromException(new InvalidOperationException("the browser is gone")));
        int deletes = 0;

        bool done = await WebWipe.RunAsync(
            Ended,
            Now,
            [earlier, mine],
            () =>
            {
                deletes++;
                Expect.That(mine.Closed, "the folder is deleted while a web view of it is still open");
                return Task.FromResult(false);
            },
            Brief).WaitAsync(Long);

        Expect.That(!done, "the session's data was taken for gone because an earlier session's profile could be cleared");
        Expect.That(!earlier.AskedToClear, "an earlier session's web view was asked to clear for this one");
        Expect.That(earlier.Closed, "an earlier session's web view is still alive");
        Expect.That(mine.Closed, "the web view of the session that ends is still alive");
        Assert.AreEqual(1, deletes, "the folder of the session was not tried when its profile could not be cleared");
    }

    [TestMethod]
    public async Task When_no_web_view_of_the_session_can_clear_its_profile_the_folder_decides_and_every_web_view_is_closed_first()
    {
        FakeWebView first = new("the first page", Ended, Never);
        FakeWebView second = new("the second page", Ended, Never);
        bool gone = await WebWipe.RunAsync(Ended, Now, [first, second], () => Task.FromResult(first.Closed && second.Closed), Brief).WaitAsync(Long);
        Expect.That(gone, "the folder was asked to go while a web view of the session was open, or its answer was not taken");
        Expect.That(first.AskedToClear && second.AskedToClear, "the second web view was not asked after the first did not answer");

        // The folder cannot be deleted either: the session's web data is still there, and the wipe says so.
        FakeWebView last = new("the last page", Ended, Never);
        Expect.That(!await WebWipe.RunAsync(Ended, Now, [last], () => Task.FromResult(false), Brief).WaitAsync(Long), "data that could be neither cleared nor deleted is taken for gone");
        Expect.That(!await WebWipe.RunAsync(Ended, Now, [], () => Task.FromException<bool>(new IOException("held")), Brief).WaitAsync(Long), "a delete that failed is taken for done");
    }

    [TestMethod]
    public async Task A_web_view_of_whoever_comes_next_and_one_that_was_never_started_are_left_as_they_are()
    {
        // The folder has moved on already: a page asked for during the wipe was started in the new folder.
        FakeWebView next = new("the next person's page", Now);
        FakeWebView unstarted = new("a page that was never started", null);
        FakeWebView mine = new("the page of the person who left", Ended);

        bool done = await WebWipe.RunAsync(Ended, Now, [next, unstarted, mine], () => Task.FromResult(true), Brief).WaitAsync(Long);

        Expect.That(done && mine.Closed, "the session's own web view was not cleared and closed");
        Expect.That(!next.Closed && !next.Emptied && !next.AskedToClear, "the web view of whoever comes next was touched by the sign-out of the person before");
        Expect.That(!unstarted.Closed && !unstarted.AskedToClear, "a web view that was never started was touched");

        // With no folder for whoever comes next (no web page may be shown), every web view that is not the session's own is an older one.
        FakeWebView other = new("a web view of another folder", Now);
        _ = await WebWipe.RunAsync(Ended, null, [other], () => Task.FromResult(true), Brief).WaitAsync(Long);
        Expect.That(other.Closed, "a web view was left alive though no web page may be shown");
    }

    [TestMethod]
    public async Task WebPagesUnderTheSignInAreTakenDown()
    {
        Origins origins = new("https://shop.example");
        string transcripts = Path.Combine(Repository.Root, "crates", "navree-native-core", "transcripts");
        string shellBody = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", "made", "shell-routes.json"));
        string legacyHtml = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", "made", "legacy.html"));
        string endedBody = await File.ReadAllTextAsync(Path.Combine(transcripts, "fixtures", "made", "session-ended.json"));
        byte[] config = Encoding.UTF8.GetBytes("""{"origin":"https://shop.example","client":{"platform":"windows","form":"desktop","vocab":0,"text_percent":100}}""");

        using CoreClient client = new(config, _ => { });
        _ = await client.SendAsync(Events.Start);
        Update shell = await client.SendAsync(Events.Response(1, 200, "application/vnd.navree.native+json", null, [], shellBody));
        Update legacy = await client.SendAsync(Events.Response(2, 200, "text/html; charset=utf-8", null, [], legacyHtml));
        Expect.That(legacy.State.Screens[1].Content is WebContent, "the first screen did not open in the web view");

        // While the screen is shown with no cover, its web view stands and its requests to the app's origin go out.
        FakeWebView mainWeb = new("main window web page", Now, screen: 1);
        Assert.AreEqual(0, WebWipe.TakeDownUnderCover(legacy.State, [mainWeb]));
        Assert.IsFalse(mainWeb.Emptied || mainWeb.Closed);
        Assert.AreEqual(WebAnswer.Ours, WebData.Answer("https://shop.example/legacy.js", origins.Ours, wiping: false, covered: !CoverRules.AllowsWeb(legacy.State, 1)));

        // Refreshing the orders destination answers 401 and puts the sign-in cover up: the core parks the web screen underneath, and the
        // client empties and closes every web view under the cover (main window and secondary window alike), while keeping a web sign-in
        // cover's own web view alive until the sign-in finishes.
        Update selected = await client.SendAsync(Events.Select(1));
        long ordersRequest = selected.Effects.OfType<FetchEffect>().Single().Request;
        Update covered = await client.SendAsync(Events.Response(ordersRequest, 401, "application/vnd.navree.native+json", null, [], endedBody));
        Expect.That(CoverRules.IsCovered(covered.State), "the 401 did not put a locked cover up");
        long coverScreen = CoverRules.Decide(covered.State).CoverScreenId!.Value;
        long signInRequest = covered.Effects.OfType<FetchEffect>().Single().Request;

        // Suppose the sign-in page itself is HTML in the web view, while another web view stood in a secondary window.
        Update htmlSignIn = await client.SendAsync(Events.Response(signInRequest, 200, "text/html; charset=utf-8", null, [], "<html><body>Sign in</body></html>"));
        FakeWebView secondaryWeb = new("secondary window web page", Now, screen: 2);
        FakeWebView signInWeb = new("sign-in cover web page", Now, screen: coverScreen);
        int takenDown = WebWipe.TakeDownUnderCover(htmlSignIn.State, [mainWeb, secondaryWeb, signInWeb]);
        Assert.AreEqual(2, takenDown, "every web view under the sign-in cover must be taken down while the HTML sign-in cover itself stays");
        Expect.That(mainWeb.Emptied && mainWeb.Closed, "the main window's web view under the sign-in cover was left alive");
        Expect.That(secondaryWeb.Emptied && secondaryWeb.Closed, "the secondary window's web view under the sign-in cover was left alive");
        Expect.That(!signInWeb.Emptied && !signInWeb.Closed, "the HTML sign-in cover's own web view was closed before the person could sign in");
        Assert.AreEqual(WebAnswer.Refused, WebData.Answer("https://shop.example/api/poll", origins.Ours, wiping: false, covered: !CoverRules.AllowsWeb(htmlSignIn.State, 1)));

        // Even if a state still held WebContent on a destination screen while a locked cover stood over it, AllowsWeb refuses it.
        ViewState unparkedUnderCover = htmlSignIn.State with
        {
            Screens = htmlSignIn.State.Screens.SetItem(1, new ScreenView { Id = 1, Url = "https://shop.example/legacy", Content = new WebContent("https://shop.example/legacy", "Legacy", [], 1) }),
        };
        FakeWebView stillWeb = new("unparked web view under cover", Now, screen: 1);
        Assert.AreEqual(1, WebWipe.TakeDownUnderCover(unparkedUnderCover, [stillWeb]));
        Expect.That(stillWeb.Emptied && stillWeb.Closed, "a web screen underneath a locked cover was left alive");

        // Once the sign-in cover finishes (for example via a web link back to the app) and turns into the locked waiting cover while the
        // shell is asked, the sign-in web view itself is taken down too.
        Update waiting = await client.SendAsync(Events.WebLink(coverScreen, "https://shop.example/orders"));
        Expect.That(CoverRules.IsCovered(waiting.State), "the post-sign-in waiting cover is not up while the shell is asked");
        Assert.AreEqual(1, WebWipe.TakeDownUnderCover(waiting.State, [signInWeb]));
        Expect.That(signInWeb.Emptied && signInWeb.Closed, "the web view of the sign-in cover stayed alive under the waiting cover");
        _ = shell;
    }
}
