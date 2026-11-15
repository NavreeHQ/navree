using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Navree.Design;
using Navree.Protocol;
using Navree.Runtime;
using Windows.Graphics;
using Windows.System;

namespace Navree.Shell;

/// <summary>
/// The app's window: the title bar with its back button, Mica behind everything, the navigation by the window's
/// width, and the screen in front. It shows the host's view state and tells the host what the person did.
/// </summary>
public sealed partial class ShellWindow : Window
{
    private readonly ShellHost host;
    private readonly TitleBar titleBar = new();
    private readonly NavigationView navigation = new();
    private readonly ScreenPresenter front;
    private readonly ScreenPresenter back;
    private readonly Grid content = new();
    private readonly StackPanel messages = new() { Spacing = 8, VerticalAlignment = VerticalAlignment.Bottom, HorizontalAlignment = HorizontalAlignment.Center, Margin = new Thickness(16), MaxWidth = 560 };
    private readonly TextBlock announcer = new() { Opacity = 0, Height = 0, IsHitTestVisible = false };
    private readonly Grid root = new();
    private readonly Dialogs dialogs;
    private readonly SidePane sidePane = new();
    private readonly AutoSuggestBox search = new() { Visibility = Visibility.Collapsed, MaxWidth = 460, VerticalAlignment = VerticalAlignment.Center, QueryIcon = new SymbolIcon(Symbol.Find) };
    private bool updatingSelection;
    private string destinationsKey = string.Empty;
    private WidthClass width = WidthClass.Expanded;
    private bool widthKnown;

    /// <summary>Makes the window for an app's host.</summary>
    /// <param name="host">The app's host.</param>
    public ShellWindow(ShellHost host)
    {
        ArgumentNullException.ThrowIfNull(host);
        this.host = host;
        front = new ScreenPresenter(host);
        back = new ScreenPresenter(host);
        dialogs = new Dialogs(host, root, sidePane);

        Title = host.Options.AppName;
        SystemBackdrop = new MicaBackdrop { Kind = Microsoft.UI.Composition.SystemBackdrops.MicaKind.Base };
        ExtendsContentIntoTitleBar = true;
        AppWindow.TitleBar.PreferredHeightOption = TitleBarHeightOption.Tall;

        titleBar.Title = host.Options.AppName;
        titleBar.IsBackButtonVisible = true;
        titleBar.IsBackButtonEnabled = false;
        titleBar.IsPaneToggleButtonVisible = true;
        titleBar.Content = search;
        search.QuerySubmitted += (_, e) =>
        {
            if (host.State.FrontScreen is ScreenView current)
            {
                host.Send(Events.Search(current.Id, e.QueryText));
            }
        };
        titleBar.BackRequested += (_, _) =>
        {
            if (settingsOpen)
            {
                HideSettings();
            }
            else
            {
                host.Send(Events.Back);
            }
        };
        titleBar.PaneToggleRequested += (_, _) => navigation.IsPaneOpen = !navigation.IsPaneOpen;

        navigation.IsBackButtonVisible = NavigationViewBackButtonVisible.Collapsed;
        navigation.IsPaneToggleButtonVisible = false;
        navigation.IsSettingsVisible = false;
        navigation.PaneDisplayMode = NavigationViewPaneDisplayMode.Auto;
        navigation.SelectionChanged += OnSelectionChanged;

        content.Children.Add(front);
        content.Children.Add(sidePane);
        content.Children.Add(messages);
        content.Children.Add(announcer);
        navigation.Content = content;

        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        root.Children.Add(titleBar);
        Grid.SetRow(navigation, 1);
        root.Children.Add(navigation);
        Content = root;
        SetTitleBar(titleBar);

        root.SizeChanged += (_, e) => OnWidth(e.NewSize.Width);
        root.KeyboardAccelerators.Add(Accelerator(VirtualKey.F5, VirtualKeyModifiers.None, () =>
        {
            if (host.State.Nav.Front is long screen)
            {
                host.Send(Events.Refresh(screen));
            }
        }));
        root.KeyboardAccelerators.Add(Accelerator(VirtualKey.Left, VirtualKeyModifiers.Menu, () => host.Send(Events.Back)));

        host.StateChanged += Show;
        host.MessageRequested += ShowMessage;
        host.Announcement += Announce;
        Closed += (_, _) =>
        {
            host.StateChanged -= Show;
            host.MessageRequested -= ShowMessage;
            host.Announcement -= Announce;
            host.CoreStopped -= OfferRestart;
        };
        InitializeSystem();
    }

    /// <summary>The element the window's pictures are taken of: everything but the system's own frame.</summary>
    public FrameworkElement Surface => root;

    /// <summary>What is over the window (a question, a dialog, a pane at the side), by name; null when nothing is.</summary>
    public string? OverTheWindow => dialogs.ShowingKey;

    /// <summary>The page of the screen in front, all of it (not only what the window's height shows).</summary>
    public FrameworkElement? CurrentPage => front.PageElement;

    /// <summary>Sets the direction text runs in, for a language that runs right to left.</summary>
    /// <param name="rightToLeft">Whether it runs from the right.</param>
    public void SetFlowDirection(bool rightToLeft)
    {
        root.FlowDirection = rightToLeft ? FlowDirection.RightToLeft : FlowDirection.LeftToRight;
        MirrorFrame(rightToLeft);
    }

    /// <summary>Sizes the window, in effective pixels, and gives its smallest.</summary>
    /// <param name="widthEpx">The width.</param>
    /// <param name="heightEpx">The height.</param>
    public void Size(int widthEpx, int heightEpx)
    {
        double scale = root.XamlRoot?.RasterizationScale ?? 1.0;
        AppWindow.Resize(new SizeInt32((int)(widthEpx * scale), (int)(heightEpx * scale)));
        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.PreferredMinimumWidth = (int)(WindowRules.SmallestWindow * scale);
        }
    }

    private static KeyboardAccelerator Accelerator(VirtualKey key, VirtualKeyModifiers modifiers, Action run)
    {
        KeyboardAccelerator accelerator = new() { Key = key, Modifiers = modifiers };
        accelerator.Invoked += (_, e) =>
        {
            run();
            e.Handled = true;
        };
        return accelerator;
    }

    // The window's width class is the core's to know: it changes the placement of what the core draws.
    private void OnWidth(double effective)
    {
        WidthClass now = WindowRules.ClassOf(effective);
        if (widthKnown && now == width)
        {
            return;
        }

        widthKnown = true;
        width = now;
        host.Send(Events.Window(now switch
        {
            WidthClass.Compact => Runtime.Width.Compact,
            WidthClass.Medium => Runtime.Width.Medium,
            _ => Runtime.Width.Expanded,
        }));
        Show(host.State);
    }

    private void OnSelectionChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (updatingSelection)
        {
            return;
        }

        if (args.SelectedItemContainer?.Tag is int index && index != host.State.Nav.Selected)
        {
            host.Send(Events.Select(index));
        }
    }

    /// <summary>Shows the view state: the navigation, the screen or screens in front, what is over them.</summary>
    private void Show(ViewState state)
    {
        NavPart nav = state.Nav;
        titleBar.IsBackButtonEnabled = nav.CanGoBack || settingsOpen;
        ShowDestinations(nav);
        ShowSearch(state.FrontScreen?.Screen?.Search);

        if (nav.Stage != Stage.Ready)
        {
            ShowStage(nav);
            return;
        }

        // The screens of the selected destination: the one in front, and under it its list when the window is wide enough for both.
        Destination? selected = nav.Selected >= 0 && nav.Selected < nav.Destinations.Count ? nav.Destinations[nav.Selected] : null;
        IReadOnlyList<long> stack = selected?.Stack ?? [];
        bool modalInFront = nav.Modals.Count > 0 && nav.Front is long frontId && nav.Modals.Any(modal => modal.Stack.Contains(frontId));
        long? baseId = stack.Count > 0 ? stack[^1] : null;
        ScreenView? baseScreen = baseId is long b && state.Screens.TryGetValue(b, out ScreenView? held) ? held : null;

        bool twoPanes = !modalInFront && WindowRules.TwoPanes(width, stack.Count);
        if (twoPanes)
        {
            long under = stack[^2];
            back.Show(state.Screens.TryGetValue(under, out ScreenView? underScreen) ? underScreen : null, width);
            EnsurePanes(true);
        }
        else
        {
            EnsurePanes(false);
        }

        front.Show(baseScreen, width);
        dialogs.Update(state, front.Context, modalInFront);
    }

    // The screen's own search is the title bar's: the place a Windows person looks for it.
    private void ShowSearch(Models.Search? wanted)
    {
        search.Visibility = wanted is null ? Visibility.Collapsed : Visibility.Visible;
        if (wanted is null)
        {
            return;
        }

        search.PlaceholderText = wanted.Placeholder ?? "Search";
        if (search.FocusState == FocusState.Unfocused && search.Text != wanted.Value)
        {
            search.Text = wanted.Value;
        }

        AutomationProperties.SetName(search, wanted.Placeholder ?? "Search");
    }

    private void ShowStage(NavPart nav)
    {
        front.Show(null, width);
        EnsurePanes(false);
        Grid stage = new() { Padding = new Thickness(32) };
        StackPanel panel = new() { Spacing = 12, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center, MaxWidth = 480 };
        switch (nav.Stage)
        {
            case Stage.Failed:
                (string title, string help) = Words.Failure(nav.Failure?.ToString().ToLowerInvariant() ?? string.Empty);
                panel.Children.Add(Look.Label(title, "SubtitleTextBlockStyle"));
                panel.Children.Add(Look.Label(help, muted: true));
                Button again = new() { Content = "Try again" };
                again.Click += (_, _) => host.Send(Events.Start);
                panel.Children.Add(again);
                break;
            case Stage.Upgrade:
                panel.Children.Add(Look.Label("This app needs an update", "SubtitleTextBlockStyle"));
                panel.Children.Add(Look.Label("The server needs a newer version of this app than the one you have.", muted: true));
                if (!string.IsNullOrEmpty(nav.Upgrade?.Url))
                {
                    Button update = new() { Content = "Get the update" };
                    update.Click += (_, _) => host.Send(Events.OpenUpgrade);
                    panel.Children.Add(update);
                }

                break;
            default:
                panel.Children.Add(new ProgressRing { IsActive = true, Width = 40, Height = 40, HorizontalAlignment = HorizontalAlignment.Center });
                panel.Children.Add(Look.Label(host.Options.AppName, "BodyTextBlockStyle", muted: true));
                break;
        }

        stage.Children.Add(panel);
        navigation.Content = stage;
        stagePanel = stage;
    }

    private UIElement? stagePanel;
    private Grid? panes;

    // A list and its detail side by side: two presenters in a grid in place of one.
    private void EnsurePanes(bool two)
    {
        if (stagePanel is not null)
        {
            stagePanel = null;
            navigation.Content = content;
        }

        if (two && panes is null)
        {
            content.Children.Remove(front);
            panes = new Grid { ColumnSpacing = 1 };
            panes.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(WindowRules.ListPane) });
            panes.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
            panes.Children.Add(back);
            Grid.SetColumn(front, 1);
            panes.Children.Add(front);
            content.Children.Insert(0, panes);
        }
        else if (!two && panes is not null)
        {
            panes.Children.Clear();
            content.Children.Remove(panes);
            panes = null;
            Grid.SetColumn(front, 0);
            content.Children.Insert(0, front);
        }
    }

    // The destinations are made again only when what they are changed.
    private void ShowDestinations(NavPart nav)
    {
        string key = string.Join("|", nav.Destinations.Select(destination => destination.Label + "\u001f" + destination.Href + "\u001f" + destination.Badge));
        updatingSelection = true;
        try
        {
            if (key != destinationsKey)
            {
                destinationsKey = key;
                navigation.MenuItems.Clear();
                for (int at = 0; at < nav.Destinations.Count; at++)
                {
                    Destination destination = nav.Destinations[at];
                    NavigationViewItem item = new()
                    {
                        Content = destination.Label,
                        Icon = Look.Glyph(SystemGlyphs.For(destination.Icon?.Name, destination.Label)),
                        Tag = at,
                    };
                    if (destination.Badge is not null)
                    {
                        item.InfoBadge = new InfoBadge { Value = int.TryParse(destination.Badge, out int count) ? count : 0, Visibility = int.TryParse(destination.Badge, out _) ? Visibility.Visible : Visibility.Collapsed };
                    }

                    navigation.MenuItems.Add(item);
                }
            }

            if (!settingsOpen && nav.Selected >= 0 && nav.Selected < navigation.MenuItems.Count && !ReferenceEquals(navigation.SelectedItem, navigation.MenuItems[nav.Selected]))
            {
                navigation.SelectedItem = navigation.MenuItems[nav.Selected];
            }
        }
        finally
        {
            updatingSelection = false;
        }
    }

    // ---- messages and announcements -------------------------------------------------------------------

    private void ShowMessage(Message message)
    {
        string text = message.Text ?? (message.Notice is Notice notice ? Words.Notice(NoticeName(notice)) : null) ?? string.Empty;
        InfoBar bar = new()
        {
            IsOpen = true,
            IsClosable = true,
            Severity = Look.Severity(message.Tone),
            Title = message.Title ?? string.Empty,
            Message = text,
        };
        if (message.Action is not null)
        {
            Button action = new() { Content = message.Action };
            action.Click += (_, _) =>
            {
                host.Send(Events.MessageAction(message.Id));
                bar.IsOpen = false;
            };
            bar.ActionButton = action;
        }

        bar.Closed += (_, _) =>
        {
            messages.Children.Remove(bar);
            host.Send(Events.MessageShown(message.Id));
        };
        messages.Children.Add(bar);

        // A failure stays until it is closed; a word of good news goes by itself.
        if (message.Tone is Models.Tone.Success or Models.Tone.Neutral or Models.Tone.Info && message.Action is null)
        {
            DispatcherTimer timer = new() { Interval = TimeSpan.FromSeconds(5) };
            timer.Tick += (_, _) =>
            {
                timer.Stop();
                bar.IsOpen = false;
            };
            timer.Start();
        }

        Announce(string.Join(". ", new[] { message.Title, text }.Where(part => !string.IsNullOrEmpty(part))));
    }

    private static string NoticeName(Notice notice) =>
        notice switch
        {
            Notice.Offline => "offline",
            Notice.Timeout => "timeout",
            Notice.Failed => "failed",
            Notice.Unsaved => "unsaved",
            Notice.Loading => "loading",
            Notice.Copied => "copied",
            Notice.FieldsNeedAttention => "fields_need_attention",
            Notice.Loaded => "loaded",
            Notice.SecondStep => "second_step",
            _ => string.Empty,
        };

    // What assistive technology is told, without moving the focus.
    private void Announce(string text)
    {
        if (string.IsNullOrEmpty(text))
        {
            return;
        }

        announcer.Text = text;
        AutomationPeer? peer = FrameworkElementAutomationPeer.FromElement(announcer) ?? FrameworkElementAutomationPeer.CreatePeerForElement(announcer);
        peer?.RaiseNotificationEvent(AutomationNotificationKind.Other, AutomationNotificationProcessing.ImportantMostRecent, text, "navree");
    }
}
