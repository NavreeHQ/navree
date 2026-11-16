using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Navree.Design;
using Navree.Design.Pages;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>
/// Shows one screen of the view state. When an answer changes the screen, the page that is drawn is brought up to date in place: a part stands if what
/// it was drawn from did not change, a part that holds the person's own (a box with the focus, a table with its place) is given what changed and keeps
/// the rest, and only a part that cannot be is drawn again, alone. The whole page is drawn again only for another screen or another width.
/// </summary>
internal sealed class ScreenPresenter : Grid
{
    private readonly ShellHost host;
    private readonly ScrollViewer scroller = new() { VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
    private readonly Dictionary<long, WebPage> webs = [];
    private readonly Dictionary<long, double> offsets = [];
    private readonly PageKeeper keeper = new();
    private readonly TableLayouts layouts;
    private ScreenView? shown;
    private WidthClass shownWidth;
    private NodeRenderer? renderer;
    private PageParts? parts;

    internal ScreenPresenter(ShellHost host)
    {
        this.host = host;
        layouts = new TableLayouts(host);
        Children.Add(scroller);
        scroller.SizeChanged += (_, _) => FitPage();
    }

    // The page is as wide as the room the window gives it, up to its widest, and is placed in the middle by margins of our own: left to be centred by the
    // layout alone, a wide window (full screen) was seen to place it right of the middle and run it off the edge.
    private void FitPage()
    {
        if (scroller.Content is StackPanel page)
        {
            double room = scroller.ViewportWidth > 0 ? scroller.ViewportWidth : scroller.ActualWidth;
            if (room > 0)
            {
                page.Width = Math.Min(page.MaxWidth, room);

                // What the window has beyond the page's widest is left evenly at both sides, so a wide window shows the page in the middle.
                double side = Math.Max(0, Math.Floor((room - page.Width) / 2));
                page.Margin = new Thickness(side, 0, side, 0);
            }
        }
    }

    /// <summary>The page the screen was drawn into, all of it: the element a picture of the whole page is taken of.</summary>
    internal FrameworkElement? PageElement => scroller.Content as FrameworkElement;

    /// <summary>What drawing the screen found and drew; null while nothing is shown.</summary>
    internal RenderContext? Context { get; private set; }

    /// <summary>The parts of the page that is drawn: the element they are all in, and the record of the body.</summary>
    private sealed class PageParts
    {
        internal required StackPanel Panel { get; init; }

        internal required Drawn Record { get; init; }

        internal ProgressBar? Progress { get; set; }

        internal UIElement Header { get; set; } = null!;

        internal List<Drawn> HeaderRecords { get; } = [];

        internal UIElement? FailureBar { get; set; }

        internal Failure? FailureShown { get; set; }

        internal Drawn Body { get; set; } = null!;
    }

    /// <summary>Shows a screen, or nothing.</summary>
    internal void Show(ScreenView? screen, WidthClass width)
    {
        if (screen is null)
        {
            shown = null;
            Context = null;
            renderer = null;
            parts = null;
            scroller.Content = null;
            Present(scroller);
            return;
        }

        if (screen.Content is WebContent web)
        {
            if (webs.TryGetValue(screen.Id, out WebPage? page) && page.OfAnEndedSession && !page.Shows(web))
            {
                // A web view started for a session that has ended is never shown a page again (the page it was emptied of stays empty): when
                // the screen has another page to show it gets a new web view, which keeps its data in the folder of the session now.
                webs.Remove(screen.Id);
                page.Release();
                page = null;
            }

            if (page is null)
            {
                page = new WebPage(host, screen.Id, host.RecordedPages);
                webs[screen.Id] = page;
            }

            shown = screen;
            Context = null;
            renderer = null;
            parts = null;
            Present(page);
            _ = page.ShowAsync(web);
            return;
        }

        Present(scroller);

        // Where the person was on this page, before anything is changed: the focus and what it holds.
        PageKeeper.Held? held = shown is not null && shown.Id == screen.Id ? PageKeeper.Capture(scroller.Content as FrameworkElement) : null;
        if (shown is not null && shown.Id == screen.Id && shownWidth == width && BringUpToDate(screen, held))
        {
            shown = screen;
            return;
        }

        DrawWhole(screen, width, held);
    }

    // The screen is the one on the screen at the width it is drawn for: what changed is brought up to date in the page that stands; false when the page
    // has to be drawn again (the screen shows something else than a native page, or a part could not be put in its place).
    private bool BringUpToDate(ScreenView screen, PageKeeper.Held? held)
    {
        if (renderer is null || Context is not { } context || parts is not { } page
            || screen.Content is not NativeContent native || shown?.Content is not NativeContent before || before.Screen is not { } was)
        {
            return false;
        }

        _ = context.TakeCount();
        int lazy = context.TakeLazy();
        double offset = scroller.VerticalOffset;
        double extentBefore = scroller.ExtentHeight;
        bool earlierArrived = EarlierArrived(shown, screen);
        context.Screen = screen;
        renderer.Stood = 0;
        renderer.Updated = 0;
        renderer.Replaced = 0;
        renderer.Structural = 0;
        renderer.Failed = false;
        Models.Screen now = native.Screen;
        bool pageChanged = false;

        bool wantsProgress = screen.Loading || screen.Refreshing;
        if (wantsProgress != (page.Progress is not null))
        {
            if (wantsProgress)
            {
                page.Progress = new ProgressBar { IsIndeterminate = true };
                page.Panel.Children.Insert(0, page.Progress);
            }
            else
            {
                _ = page.Panel.Children.Remove(page.Progress!);
                page.Progress = null;
            }

            pageChanged = true;
        }

        if (!HeaderStands(page, was, now, shown!, screen))
        {
            int at = page.Panel.Children.IndexOf(page.Header);
            foreach (Drawn gone in page.HeaderRecords)
            {
                _ = page.Record.Children.Remove(gone);
            }

            page.HeaderRecords.Clear();
            page.Header = Header(now, context, node => HeaderPart(page, node));
            if (at >= 0)
            {
                page.Panel.Children[at] = page.Header;
            }

            pageChanged = true;
        }

        if (screen.Failure != page.FailureShown)
        {
            if (page.FailureBar is not null)
            {
                _ = page.Panel.Children.Remove(page.FailureBar);
            }

            page.FailureShown = screen.Failure;
            page.FailureBar = screen.Failure is Failure failure ? FailureBar(failure, screen.Id, retry: true) : null;
            if (page.FailureBar is not null)
            {
                page.Panel.Children.Insert(page.Panel.Children.IndexOf(page.Header) + 1, page.FailureBar);
            }

            pageChanged = true;
        }

        // The words of the title decide which first heading is left out: when the title changes, the body is drawn again.
        _ = was.Title == now.Title
            ? renderer.Refresh(page.Body, now.Body)
            : renderer.Redraw(page.Body, now.Body);
        if (renderer.Failed)
        {
            return false;
        }

        context.Rebuild();
        context.ShowFields();
        (int rendered, string kinds) = context.TakeCount();
        PageReports.Add(new PageReport(screen.Id, "update", rendered, renderer.Stood, renderer.Updated, renderer.Replaced, kinds, lazy));
        bool replaced = pageChanged || renderer.Replaced > 0 || renderer.Structural > 0;
        if (replaced || earlierArrived)
        {
            scroller.UpdateLayout();
            if (earlierArrived)
            {
                offset += Math.Max(0, scroller.ExtentHeight - extentBefore);
                offsets[screen.Id] = offset;
            }

            scroller.ChangeView(null, offset, null, disableAnimation: true);
        }

        // Where the focus was is carried to the same part of what was drawn again, when what held it was.
        if (held is not null && replaced && !PageKeeper.HoldsFocus(page.Panel))
        {
            PageKeeper.Restore(page.Panel, held);
        }

        keeper.Serve(page.Panel, screen, host.Send, () => Context is { } live ? live.OverlayRoots() : []);

        // A card that was just moved gets its handle back by the id the focus is put back by; the wish for it does not outlive this answer.
        string? wanted = CardFocus.Wanted;
        if (wanted is not null)
        {
            page.Panel.DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, async () =>
            {
                await Task.Delay(1200);
                if (CardFocus.Wanted == wanted)
                {
                    CardFocus.Wanted = null;
                }
            });
        }

        return true;
    }

    private UIElement HeaderPart(PageParts page, Models.Node node)
    {
        Drawn record = renderer!.RenderUnder(page.Record, node);
        page.HeaderRecords.Add(record);
        return record.Element;
    }

    // The title, the words under it and the actions are the same, and each action stands.
    private static bool HeaderStands(PageParts page, Models.Screen was, Models.Screen now, ScreenView before, ScreenView after)
    {
        if (was.Title != now.Title || was.Subtitle != now.Subtitle || page.HeaderRecords.Count != now.Actions.Count)
        {
            return false;
        }

        ScreenFacts oldFacts = new(before, fields: false);
        ScreenFacts newFacts = new(after, fields: false);
        for (int at = 0; at < now.Actions.Count; at++)
        {
            Drawn record = page.HeaderRecords[at];
            if (!PageRules.Same(record.Node, now.Actions[at]) || !PageRules.SameAround(now.Actions[at], oldFacts, newFacts))
            {
                return false;
            }
        }

        return true;
    }

    // Earlier rows that have just arrived above a list push what the person was reading down: the view is moved by what they added, so the
    // same rows stay where they were under the eye.
    private static bool EarlierArrived(ScreenView? previous, ScreenView screen) =>
        previous is not null && previous.Id == screen.Id
        && previous.Lists.Any(pair => pair.Value.LoadingBefore && screen.Lists.TryGetValue(pair.Key, out ListState? now) && !now.LoadingBefore && !now.FailedBefore);

    // The screen is drawn from its tree, all of it, the scroll position kept.
    private void DrawWhole(ScreenView screen, WidthClass width, PageKeeper.Held? held)
    {
        // The place of each screen's scroll is kept as the page is replaced.
        if (shown is not null)
        {
            offsets[shown.Id] = scroller.VerticalOffset;
        }

        foreach (long gone in offsets.Keys.Where(id => !host.State.Screens.ContainsKey(id)).ToList())
        {
            _ = offsets.Remove(gone);
            NodeRenderer.Forget(gone);
        }

        // A web screen that has left the app lets go of its web view and the process behind it.
        foreach (long gone in webs.Keys.Where(id => !host.State.Screens.ContainsKey(id)).ToList())
        {
            if (webs.Remove(gone, out WebPage? left))
            {
                left.Release();
            }
        }

        double offset = offsets.TryGetValue(screen.Id, out double kept1) ? kept1 : 0;
        double extentBefore = scroller.ExtentHeight;
        bool earlierArrived = EarlierArrived(shown, screen);
        shown = screen;
        shownWidth = width;
        RenderContext context = new()
        {
            Screen = screen,
            Send = host.Send,
            Width = width,
            Pictures = host.LoadPictureAsync,
            Host = host,
            Layouts = layouts,
            Settled = NodeRenderer.Seen(screen.Id),
        };
        Context = context;
        renderer = new NodeRenderer(context);
        parts = null;
        UIElement drawn = Page(context, renderer);
        FrameworkElement built = (FrameworkElement)drawn;
        scroller.Content = drawn;
        context.Settled = true;
        context.Rebuild();
        (int rendered, string kinds) = context.TakeCount();
        PageReports.Add(new PageReport(screen.Id, "whole", rendered, 0, 0, 0, kinds, 0));
        FitPage();
        if (offset > 0 || earlierArrived)
        {
            scroller.UpdateLayout();
            if (earlierArrived)
            {
                offset += Math.Max(0, scroller.ExtentHeight - extentBefore);
                offsets[screen.Id] = offset;
            }

            scroller.ChangeView(null, offset, null, disableAnimation: true);
        }

        if (held is not null)
        {
            PageKeeper.Restore(built, held);
        }

        keeper.Serve(built, screen, host.Send, () => Context is { } live ? live.OverlayRoots() : []);
    }

    // One thing fills the area: the screen's scrolling page, or a web view.
    private void Present(UIElement element)
    {
        if (Children.Count != 1 || !ReferenceEquals(Children[0], element))
        {
            Children.Clear();
            Children.Add(element);
        }
    }

    private UIElement Page(RenderContext context, NodeRenderer renderer)
    {
        ScreenView screen = context.Screen;
        double side = context.Width == WidthClass.Compact ? 16 : 32;
        StackPanel page = new() { Spacing = 16, Padding = new Thickness(side, 16, side, 32), MaxWidth = WindowRules.ReadingMeasure + 2 * side + 400, HorizontalAlignment = HorizontalAlignment.Left };
        ProgressBar? progress = null;
        if (screen.Loading || screen.Refreshing)
        {
            progress = new ProgressBar { IsIndeterminate = true };
            page.Children.Add(progress);
        }

        switch (screen.Content)
        {
            case NativeContent native:
                Drawn record = new(native.Screen.Body) { Screen = screen, Element = page };
                PageParts held = new() { Panel = page, Record = record, Progress = progress };
                parts = held;
                held.Header = Header(native.Screen, context, node => HeaderPart(held, node));
                page.Children.Add(held.Header);
                if (screen.Failure is Failure failure)
                {
                    held.FailureShown = failure;
                    held.FailureBar = FailureBar(failure, screen.Id, retry: true);
                    page.Children.Add(held.FailureBar);
                }

                held.Body = renderer.RenderUnder(record, native.Screen.Body);
                page.Children.Add(held.Body.Element);
                record.SwapOwn = (old, fresh) =>
                {
                    int at = ReferenceEquals(old, held.Body) ? page.Children.IndexOf(old.Element) : -1;
                    if (at < 0)
                    {
                        return false;
                    }

                    page.Children[at] = fresh.Element;
                    held.Body = fresh;
                    return true;
                };
                context.Page = record;
                break;
            case WebContent web:
                page.Children.Add(Look.Label(web.Title ?? "Web page", "TitleTextBlockStyle"));
                page.Children.Add(Look.Label("This page is shown in a web view, which this build does not draw yet.", muted: true));
                break;
            default:
                if (screen.Failure is Failure failed)
                {
                    page.Children.Add(Look.Label(screen.Title ?? string.Empty, "TitleTextBlockStyle"));
                    page.Children.Add(FailureBar(failed, screen.Id, retry: true));
                }
                else if (screen.Loading || screen.Refreshing)
                {
                    if (screen.Title is not null)
                    {
                        page.Children.Add(Look.Label(screen.Title, "TitleTextBlockStyle"));
                    }
                }

                break;
        }

        return page;
    }

    private UIElement FailureBar(Failure failure, long screen, bool retry)
    {
        (string title, string help) = Words.Failure(failure.ToString().ToLowerInvariant());
        InfoBar bar = new() { IsOpen = true, IsClosable = false, Severity = InfoBarSeverity.Error, Title = title, Message = help };
        if (retry)
        {
            Button again = new() { Content = "Try again" };
            again.Click += (_, _) => host.Send(Events.Refresh(screen));
            bar.ActionButton = again;
        }

        return bar;
    }

    private static UIElement Header(Models.Screen screen, RenderContext context, Func<Models.Node, UIElement> render)
    {
        Grid header = new() { ColumnSpacing = 16 };
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        StackPanel titles = new() { Spacing = 2 };
        TextBlock title = Look.Label(screen.Title, context.Width == WidthClass.Compact ? "SubtitleTextBlockStyle" : "TitleTextBlockStyle");
        AutomationProperties.SetHeadingLevel(title, Microsoft.UI.Xaml.Automation.Peers.AutomationHeadingLevel.Level1);
        titles.Children.Add(title);
        if (screen.Subtitle is not null)
        {
            titles.Children.Add(Look.Label(screen.Subtitle, "BodyTextBlockStyle", muted: true));
        }

        header.Children.Add(titles);
        if (screen.Actions.Count > 0)
        {
            StackPanel actions = new() { Orientation = Orientation.Horizontal, Spacing = 8, VerticalAlignment = VerticalAlignment.Top };
            foreach (Models.Node action in screen.Actions)
            {
                actions.Children.Add(render(action));
            }

            if (context.Width == WidthClass.Compact)
            {
                titles.Children.Add(actions);
            }
            else
            {
                Grid.SetColumn(actions, 1);
                header.Children.Add(actions);
            }
        }

        return header;
    }
}
