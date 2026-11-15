using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>
/// Shows one screen of the view state. When the screen is the same and only what its forms hold changed, the fields
/// show their new state in place and nothing is drawn again, so a person typing keeps their place; otherwise the
/// screen is drawn again from the tree, the scroll position kept.
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

    /// <summary>Shows a screen, or nothing.</summary>
    internal void Show(ScreenView? screen, WidthClass width)
    {
        if (screen is null)
        {
            shown = null;
            Context = null;
            scroller.Content = null;
            Present(scroller);
            return;
        }

        if (screen.Content is WebContent web)
        {
            if (!webs.TryGetValue(screen.Id, out WebPage? page))
            {
                page = new WebPage(host, screen.Id, host.RecordedPages);
                webs[screen.Id] = page;
            }

            shown = screen;
            Context = null;
            Present(page);
            _ = page.ShowAsync(web);
            return;
        }

        Present(scroller);

        bool sameDrawing = shown is not null
            && shown.Id == screen.Id
            && ReferenceEquals(shown.Content, screen.Content)
            && ReferenceEquals(shown.States, screen.States)
            && ReferenceEquals(shown.Overlays, screen.Overlays)
            && ReferenceEquals(shown.Regions, screen.Regions)
            && ReferenceEquals(shown.Lists, screen.Lists)
            && shown.Loading == screen.Loading
            && shown.Refreshing == screen.Refreshing
            && shown.Failure == screen.Failure
            && shownWidth == width;
        if (sameDrawing && Context is not null)
        {
            if (!ReferenceEquals(shown!.Forms, screen.Forms))
            {
                foreach (((string form, string field), FieldHandle handle) in Context.Fields)
                {
                    handle.Show(screen.FieldOf(form, field));
                }
            }

            shown = screen;
            if (scroller.Content is FrameworkElement kept)
            {
                keeper.Serve(kept, screen, host.Send);
            }

            return;
        }

        // Where the person was on this page, before it is replaced: the focus and what it holds, and the place of each screen's scroll.
        PageKeeper.Held? held = shown is not null && shown.Id == screen.Id ? PageKeeper.Capture(scroller.Content as FrameworkElement) : null;
        if (shown is not null)
        {
            offsets[shown.Id] = scroller.VerticalOffset;
        }

        foreach (long gone in offsets.Keys.Where(id => !host.State.Screens.ContainsKey(id)).ToList())
        {
            _ = offsets.Remove(gone);
            NodeRenderer.Forget(gone);
        }

        // A web screen that has left the app is not kept here (the web view's own closing is the web page's, which is not this file's).
        foreach (long gone in webs.Keys.Where(id => !host.State.Screens.ContainsKey(id)).ToList())
        {
            _ = webs.Remove(gone);
        }

        double offset = offsets.TryGetValue(screen.Id, out double kept1) ? kept1 : 0;

        // Earlier rows that have just arrived above a list push what the person was reading down: the view is moved by what they added, so the
        // same rows stay where they were under the eye.
        double extentBefore = scroller.ExtentHeight;
        ScreenView? previous = shown;
        bool earlierArrived = previous is not null && previous.Id == screen.Id
            && previous.Lists.Any(pair => pair.Value.LoadingBefore && screen.Lists.TryGetValue(pair.Key, out ListState? now) && !now.LoadingBefore && !now.FailedBefore);
        shown = screen;
        shownWidth = width;
        RenderContext context = new()
        {
            Screen = screen,
            Send = host.Send,
            Width = width,
            Pictures = host.LoadPictureAsync,
            Layouts = layouts,
        };
        Context = context;
        UIElement drawn = Page(context);
        FrameworkElement built = (FrameworkElement)drawn;
        scroller.Content = drawn;
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

        keeper.Serve(built, screen, host.Send);
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

    private UIElement Page(RenderContext context)
    {
        ScreenView screen = context.Screen;
        double side = context.Width == WidthClass.Compact ? 16 : 32;
        StackPanel page = new() { Spacing = 16, Padding = new Thickness(side, 16, side, 32), MaxWidth = WindowRules.ReadingMeasure + 2 * side + 400, HorizontalAlignment = HorizontalAlignment.Left };
        if (screen.Loading || screen.Refreshing)
        {
            page.Children.Add(new ProgressBar { IsIndeterminate = true });
        }

        switch (screen.Content)
        {
            case NativeContent native:
                NodeRenderer renderer = new(context);
                page.Children.Add(Header(native.Screen, renderer, context));
                if (screen.Failure is Failure failure)
                {
                    page.Children.Add(FailureBar(failure, screen.Id, retry: true));
                }

                page.Children.Add(renderer.Render(native.Screen.Body));
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

    private static UIElement Header(Models.Screen screen, NodeRenderer renderer, RenderContext context)
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
                actions.Children.Add(renderer.Render(action));
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
