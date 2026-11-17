using Microsoft.UI.Dispatching;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Navree.Presence;
using Navree.Protocol;
using Navree.Runtime;
using Windows.Graphics;

namespace Navree.Shell;

/// <summary>
/// The app's place in Windows, outside its window's content: the icon in the notification area and the small window by it, opening at
/// sign-in, the count on the taskbar button, notifications, the jump list, the share sheet, more windows, and what comes back to the running
/// app from a second start. Each is offered only where this run can do it (<see cref="Offers"/>), and every call that needs package identity is
/// made only with it. What each one answered is kept in <see cref="Log"/>, so a run says what it proved and what it only called.
/// </summary>
public sealed class SystemPlace : IDisposable
{
    private const string PendingKey = "pending-notifications";
    private readonly ShellHost host;
    private readonly ShellWindow main;
    private readonly nint mainHandle;
    private readonly DispatcherQueue dispatcher;
    private readonly bool hasIdentity;
    private readonly ChoicesFile choicesFile;
    private readonly BadgeSurface badge;
    private readonly StartupSurface startup;
    private readonly ShareSurface share;
    private readonly DispatcherQueueTimer timer;
    private readonly DispatcherQueueTimer snapshotTimer;
    private readonly List<ScreenWindow> screens = [];
    private readonly List<string> log = [];
    private NotificationSchedule schedule = new();
    private TrayIcon? tray;
    private TrayWindow? small;
    private nint appIcon;
    private nint trayIcon;
    private string placesKey = string.Empty;
    private bool quitting;
    private LaunchRequest? deferred;
    private bool changedSinceSnapshot;

    /// <summary>Makes the app's place for its main window.</summary>
    /// <param name="host">The app's host.</param>
    /// <param name="main">The main window.</param>
    public SystemPlace(ShellHost host, ShellWindow main)
    {
        ArgumentNullException.ThrowIfNull(host);
        ArgumentNullException.ThrowIfNull(main);
        this.host = host;
        this.main = main;
        dispatcher = main.DispatcherQueue;
        mainHandle = WinRT.Interop.WindowNative.GetWindowHandle(main);
        hasIdentity = PackageIdentity.Has();
        Offers = PlaceCapabilities.Current();
        string folder = host.Options.Place.ChoicesFolder ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Navree", host.Options.AppName, "place");
        choicesFile = new ChoicesFile(folder);
        Choices = choicesFile.Read();
        badge = new BadgeSurface(mainHandle, hasIdentity);
        startup = new StartupSurface(hasIdentity, host.Options.Place.StartupTaskId, host.Options.AppName);
        share = new ShareSurface(mainHandle);
        timer = dispatcher.CreateTimer();
        timer.IsRepeating = false;
        timer.Tick += (_, _) => Deliver();
        snapshotTimer = dispatcher.CreateTimer();
        snapshotTimer.Interval = TimeSpan.FromSeconds(30);
        snapshotTimer.IsRepeating = true;
        snapshotTimer.Tick += (_, _) =>
        {
            if (changedSinceSnapshot)
            {
                changedSinceSnapshot = false;
                _ = host.KeepSnapshotAsync();
            }
        };
        host.Place = this;
    }

    /// <summary>What this run offers in its place in Windows.</summary>
    public IReadOnlyList<FeatureOffer> Offers { get; }

    /// <summary>What the person chose.</summary>
    public Choices Choices { get; private set; }

    /// <summary>Whether the app opens at sign-in.</summary>
    public StartupState Startup { get; private set; }

    /// <summary>The count on the taskbar button.</summary>
    public long? Count { get; private set; }

    /// <summary>Whether the app has package identity.</summary>
    public bool HasIdentity => hasIdentity;

    /// <summary>Raised on the UI thread when something the settings page shows changed.</summary>
    public event Action? Changed;

    /// <summary>What each part of the place answered, in order, as lines.</summary>
    public IReadOnlyList<string> Log => log;

    /// <summary>How many windows besides the main window are open.</summary>
    public int ExtraWindows => screens.Count;

    /// <summary>Whether the notification-area icon is in the notification area.</summary>
    public bool TrayShown => tray?.IsAdded == true;

    /// <summary>Whether the small window by the icon is showing.</summary>
    public bool SmallWindowShown => small?.IsShown == true;

    /// <summary>
    /// Gets the place going: what the app was started with is carried out, what arrives while it runs is received, the notifications that waited are
    /// armed, and the icon is put in the notification area when the person chose it. Returns whether the main window is shown now.
    /// </summary>
    /// <param name="initial">What this start was asked.</param>
    /// <returns>Whether to show the main window: not when the system started the app at sign-in and the icon is there to open it from.</returns>
    public bool Start(LaunchRequest initial)
    {
        ArgumentNullException.ThrowIfNull(initial);
        int side = IconHandle.SmallSide();
        appIcon = IconHandle.Create(IconArt.App(side), side);
        if (appIcon != 0)
        {
            WindowFrame.SetIcon(mainHandle, appIcon);
        }

        if (host.Options.Place.Scheme is string scheme && Capabilities.Has(Offers, SystemFeature.ProtocolReturn))
        {
            bool registered = Activation.RegisterScheme(scheme, host.Options.AppName);
            Say("the app's scheme " + scheme + (registered ? " is registered" : " could not be registered"));
        }

        Activation.Receive((request, how) => dispatcher.TryEnqueue(() =>
        {
            Say("handed to the running app: " + how);
            Open(request);
        }));
        main.AppWindow.Closing += OnClosing;
        host.StateChanged += OnState;
        snapshotTimer.Start();
        host.CoreStopped += () => Say("the core stopped");

        // A sign-out the last run left unfinished was finished at this start: what the system still shows of the person who left goes too.
        if (host.FinishedSignOutAtStart)
        {
            Say("a sign-out the last run left unfinished was finished at this start");
            _ = EndSessionAsync().ContinueWith(done => dispatcher.TryEnqueue(() => Say(done.IsFaulted ? "what the system shows of the session could not all be taken away: " + done.Exception!.InnerException?.GetType().Name : "what the system showed of the session is gone")), TaskScheduler.Default);
        }

        schedule = NotificationSchedule.Read(host.Recall(PendingKey));
        if (schedule.Waiting.Count > 0)
        {
            Say(schedule.Waiting.Count + " notifications were waiting from the last run");
            Arm();
        }

        _ = ReadStartupAsync();
        if (Choices.NotificationAreaIcon && Capabilities.Has(Offers, SystemFeature.NotificationAreaIcon))
        {
            ShowIcon();
        }

        ApplyBadge();
        bool hide = initial.Background && TrayShown;
        Say(hide ? "started at sign-in: the window waits in the notification area" : "the main window is shown");
        if (initial.OpenHref is not null || initial.Return is not null || initial.NewWindow)
        {
            Open(initial with { Background = hide });
        }

        return !hide;
    }

    // ---- what the settings page shows and does ---------------------------------------------------------

    /// <summary>The cards of the settings page.</summary>
    /// <returns>The cards.</returns>
    public IReadOnlyList<Navree.Presence.SettingsCard> Cards() => SettingsRules.Cards(Offers, Choices, Startup);

    /// <summary>The lines of the About section.</summary>
    /// <returns>Pairs of a title and a line.</returns>
    public IReadOnlyList<(string Title, string Line)> AboutLines() =>
        SettingsRules.About(
            host.Options.AppName,
            typeof(SystemPlace).Assembly.GetName().Version?.ToString(3) ?? "0.1.0",
            hasIdentity,
            Offers);

    /// <summary>The person turned a switch of the settings page.</summary>
    /// <param name="id">The card's id.</param>
    /// <param name="on">Whether it is on.</param>
    public void Choose(string id, bool on)
    {
        switch (id)
        {
            case CardIds.Tray:
                Keep(Choices with { NotificationAreaIcon = on });
                if (on)
                {
                    ShowIcon();
                }
                else
                {
                    HideIcon();
                }

                break;
            case CardIds.KeepRunning:
                Keep(Choices with { KeepRunning = on });
                break;
            case CardIds.TaskbarCount:
                Keep(Choices with { TaskbarCount = on });
                ApplyBadge();
                break;
            case CardIds.Startup:
                _ = SetStartupAsync(on);
                break;
            default:
                break;
        }
    }

    /// <summary>The person pressed a button of the settings page.</summary>
    /// <param name="id">The card's id.</param>
    public void Invoke(string id)
    {
        string? address = id switch
        {
            CardIds.NotificationSettings => "ms-settings:notifications",
            CardIds.StartupSettings => "ms-settings:startupapps",
            _ => null,
        };
        if (address is not null)
        {
            _ = Windows.System.Launcher.LaunchUriAsync(new Uri(address));
        }
    }

    private void Keep(Choices next)
    {
        Choices = next;
        if (!choicesFile.Write(next))
        {
            Say("what was chosen could not be kept");
        }

        Changed?.Invoke();
    }

    private async Task ReadStartupAsync()
    {
        Startup = await startup.ReadAsync();
        Say("opening at sign-in is " + Startup + " (" + (hasIdentity ? "the startup task" : "the Run key") + ")");
        Changed?.Invoke();
    }

    private async Task SetStartupAsync(bool on)
    {
        Startup = await startup.SetAsync(on);
        Say("opening at sign-in was set " + (on ? "on" : "off") + ": it is " + Startup);
        Changed?.Invoke();
    }

    // ---- the count on the taskbar --------------------------------------------------------------------

    /// <summary>The core said what the count is (none clears it).</summary>
    /// <param name="count">The count.</param>
    public void SetBadge(long? count)
    {
        Count = BadgeRules.Shown(count);
        ApplyBadge();
    }

    private void ApplyBadge()
    {
        long? shown = Choices.TaskbarCount ? Count : null;
        bool done = badge.Show(host.Options.AppName, shown);
        Say("the count on the taskbar button is " + (BadgeRules.Label(shown) ?? "none") + ": " + (hasIdentity ? "the system's badge" : "an overlay icon") + (done ? " accepted" : " refused"));
        UpdateTrayLook();
    }

    // ---- notifications ---------------------------------------------------------------------------------

    /// <summary>The core asked for a notification, now, later or every day.</summary>
    /// <param name="notify">The call.</param>
    public void Notify(Models.Notify notify)
    {
        if (!Capabilities.Has(Offers, SystemFeature.Notifications))
        {
            Say("a notification was asked for: not shown, " + Offers.First(offer => offer.Feature == SystemFeature.Notifications).Reason);
            return;
        }

        PendingNotification? plan = NotifyRules.Plan(notify, DateTimeOffset.Now, TimeZoneInfo.Local);
        if (plan is null || !schedule.Add(plan))
        {
            Say("a notification was asked for at a time this build does not schedule, or too many wait");
            return;
        }

        Remember();
        Arm();
    }

    /// <summary>The core took a notification back.</summary>
    /// <param name="id">Its id.</param>
    public void CancelNotification(string id)
    {
        _ = schedule.Cancel(id);
        NotificationSurface.Remove(id);
        Remember();
        Arm();
    }

    /// <summary>Stops the snapshot timer while a sign-out is under way, and starts it again after.</summary>
    /// <param name="hold">Whether to stop it.</param>
    internal void HoldSnapshots(bool hold)
    {
        if (hold)
        {
            snapshotTimer.Stop();
            changedSinceSnapshot = false;
        }
        else
        {
            snapshotTimer.Start();
        }
    }

    /// <summary>
    /// The person signed out, as a step of the wipe on the UI thread: what waits to be notified, the count, what the notification centre shows and
    /// the jump list all go. Completes when the system has taken them away; fails when it could not, so the wipe reports it and the next start
    /// takes them away again.
    /// </summary>
    /// <returns>When they are gone.</returns>
    internal Task EndSessionAsync() => TakeAwayAsync("the session ended");

    /// <summary>
    /// Another person signed in with no sign-out between: what the system holds or shows for the person before goes as it does at a sign-out
    /// (what waits to be notified, the count, what the notification centre shows, the jump list), and then the jump list is written again
    /// from the destinations of the person who is here now. When this returns nothing waits to be notified and no count is shown: what the
    /// core asks for the person who is here now, from the next effect on, is theirs. Fails when the system could not take something away.
    /// </summary>
    /// <returns>When they are gone and the jump list is written again.</returns>
    internal async Task PersonChangedAsync()
    {
        try
        {
            await TakeAwayAsync("another person signed in");
        }
        finally
        {
            // The destinations of the person who is here now were shown while the list of the person before was still being emptied: they
            // are written again now, after it.
            placesKey = string.Empty;
            ShowPlaces();
        }
    }

    // What the system holds or shows for a person goes. What runs before the first wait runs at the call, on the UI thread.
    private async Task TakeAwayAsync(string why)
    {
        timer.Stop();
        schedule = new NotificationSchedule();
        Remember();
        Count = null;
        placesKey = string.Empty;
        ApplyBadge();
        if (Capabilities.Has(Offers, SystemFeature.Notifications))
        {
            await NotificationSurface.RemoveAllAsync();
        }

        if (hasIdentity && Capabilities.Has(Offers, SystemFeature.JumpList) && !await JumpListSurface.UpdateAsync([], 0))
        {
            throw new InvalidOperationException("the jump list could not be emptied");
        }

        Say(why + ": no notification waits, none is shown, no count, no jump list");
    }

    private void Remember() => host.Remember(PendingKey, schedule.Waiting.Count == 0 ? null : schedule.Write());

    // The timer is set for the next time something is due, and for no longer than an hour, so a changed clock is noticed.
    private void Arm()
    {
        timer.Stop();
        if (schedule.Next() is not DateTimeOffset next)
        {
            return;
        }

        TimeSpan wait = next - DateTimeOffset.Now;
        timer.Interval = wait < TimeSpan.FromMilliseconds(200) ? TimeSpan.FromMilliseconds(200) : wait > TimeSpan.FromHours(1) ? TimeSpan.FromHours(1) : wait;
        timer.Start();
    }

    private void Deliver()
    {
        foreach (PendingNotification due in schedule.Take(DateTimeOffset.Now, TimeZoneInfo.Local))
        {
            bool shown = NotificationSurface.Show(due);
            Say("the notification " + due.Id + (shown ? " was shown" : " could not be shown"));
        }

        Remember();
        Arm();
    }

    // ---- sharing -------------------------------------------------------------------------------------

    /// <summary>Opens the share sheet for what the core asked to share.</summary>
    /// <param name="request">What to share.</param>
    /// <param name="isOurs">Whether a whole address is the app's own.</param>
    /// <returns>Whether the person shared it, or null when the sheet could not be shown.</returns>
    public async Task<bool?> ShareAsync(Models.Share request, Func<string, bool> isOurs)
    {
        ArgumentNullException.ThrowIfNull(request);
        ArgumentNullException.ThrowIfNull(isOurs);
        Uri? link = null;
        if (request.Url is string url && Uri.TryCreate(url, UriKind.Absolute, out Uri? whole) && (whole.Scheme == Uri.UriSchemeHttps || whole.Scheme == Uri.UriSchemeHttp))
        {
            link = whole;
        }
        else if (request.Href is string href && isOurs(href) && Uri.TryCreate(href, UriKind.Absolute, out Uri? mine))
        {
            link = mine;
        }

        if (link is null && string.IsNullOrEmpty(request.Text))
        {
            return null;
        }

        bool? result = await share.ShowAsync(request.Title ?? host.Options.AppName, request.Text, link);
        Say("the share sheet answered " + (result is null ? "nothing (it could not be shown)" : result.Value ? "shared" : "not shared"));
        return result;
    }

    // ---- the windows ---------------------------------------------------------------------------------

    /// <summary>Brings the main window forward, showing it if it was hidden in the notification area.</summary>
    public void OpenMainWindow()
    {
        small?.HideNow();
        main.AppWindow.Show();
        main.Activate();
        WindowFrame.Raise(mainHandle);
    }

    /// <summary>Opens the settings page in the main window.</summary>
    public void OpenSettings()
    {
        OpenMainWindow();
        main.ShowSettings();
    }

    /// <summary>Opens a destination in the main window.</summary>
    /// <param name="index">The destination.</param>
    public void OpenDestination(int index)
    {
        host.Send(Events.Select(index));
        OpenMainWindow();
        main.HideSettings();
    }

    /// <summary>Opens the screen in front in a window of its own.</summary>
    /// <returns>Whether a window was opened.</returns>
    public bool OpenWindow()
    {
        if (host.State.Nav.Front is not long front || !host.State.Screens.ContainsKey(front))
        {
            return false;
        }

        ScreenWindow window = new(host, front, OpenMainWindow, main.IsRightToLeft);
        PointInt32 position = main.AppWindow.Position;
        SizeInt32 size = main.AppWindow.Size;
        RectInt32 at = new(position.X, position.Y, size.Width, size.Height);
        RectInt32 area = DisplayArea.GetFromWindowId(main.AppWindow.Id, DisplayAreaFallback.Nearest).WorkArea;
        Box last = screens.Count > 0
            ? new Box(screens[^1].AppWindow.Position.X, screens[^1].AppWindow.Position.Y, screens[^1].AppWindow.Position.X + screens[^1].AppWindow.Size.Width, screens[^1].AppWindow.Position.Y + screens[^1].AppWindow.Size.Height)
            : new Box(at.X, at.Y, at.X + at.Width, at.Y + at.Height);
        double scale = WindowFrame.ScaleOf(mainHandle);
        window.Place(last, new Box(area.X, area.Y, area.X + area.Width, area.Y + area.Height), (int)(Math.Max(at.Width, 560) / scale), (int)(Math.Max(at.Height, 420) / scale));
        screens.Add(window);
        window.Closed += (_, _) => screens.Remove(window);
        window.Activate();
        Say("a window of the screen " + front + " was opened beside the main one");
        return true;
    }

    /// <summary>Ends the app: what is kept for the next launch is written first, and the icon goes from the notification area.</summary>
    public void Quit() => _ = QuitAsync();

    private async Task QuitAsync()
    {
        if (quitting)
        {
            return;
        }

        quitting = true;
        try
        {
            _ = await Task.WhenAny(host.KeepSnapshotAsync(), Task.Delay(1500));
        }
        catch (Exception)
        {
            // What could not be kept is not worth keeping the person waiting.
        }

        HideIcon();
        CloseWindows();
        Application.Current.Exit();
    }

    /// <summary>Lets the app end with no question asked, and closes the windows besides the main one: the walk that pictures the app ends this way.</summary>
    public void LetExit()
    {
        quitting = true;
        CloseWindows();
    }

    // Every window of the app is closed before the app ends: one left open, hidden or not, can keep it from ending.
    private void CloseWindows()
    {
        foreach (ScreenWindow window in screens.ToArray())
        {
            try
            {
                window.Close();
            }
            catch (Exception)
            {
                // A window that is already closing is closed.
            }
        }

        screens.Clear();
        try
        {
            small?.Close();
        }
        catch (Exception)
        {
            // Likewise.
        }

        small = null;
    }

    private void OnClosing(AppWindow sender, AppWindowClosingEventArgs args)
    {
        if (quitting)
        {
            return;
        }

        args.Cancel = true;
        if (TrayRules.ClosingHides(TrayShown, Choices.KeepRunning))
        {
            sender.Hide();
            _ = host.KeepSnapshotAsync();
            Say("the window was closed and the app stays in the notification area");
        }
        else
        {
            Quit();
        }
    }

    // ---- the notification-area icon --------------------------------------------------------------------

    private void ShowIcon()
    {
        if (!Capabilities.Has(Offers, SystemFeature.NotificationAreaIcon))
        {
            return;
        }

        tray ??= MakeTray();
        int side = IconHandle.SmallSide();
        if (trayIcon == 0)
        {
            trayIcon = IconHandle.Create(IconArt.App(side), side);
        }

        bool added = tray.Show(TrayRules.Tooltip(host.Options.AppName, Count), trayIcon);
        Say("the notification-area icon " + (added ? "was added" : "was refused by the system"));
    }

    private TrayIcon MakeTray()
    {
        TrayIcon icon = new();
        icon.Selected += (_, _) => dispatcher.TryEnqueue(ToggleSmallWindow);
        // A right click opens the same small window as a click does: it is drawn by the app, so it follows the light and dark looks, which the system's own menu of the icon does not.
        icon.ContextRequested += (_, _) => dispatcher.TryEnqueue(ToggleSmallWindow);
        return icon;
    }

    private void HideIcon()
    {
        tray?.Hide();
        small?.HideNow();
    }

    private void UpdateTrayLook()
    {
        if (tray?.IsAdded == true)
        {
            _ = tray.Show(TrayRules.Tooltip(host.Options.AppName, Count), trayIcon);
        }

        small?.Refresh(Places(), Count);
    }

    /// <summary>Shows the small window by the icon, or takes it away.</summary>
    public void ToggleSmallWindow()
    {
        if (small is null)
        {
            small = new TrayWindow(this, host.Options.AppName);
            small.SetFlowDirection(main.IsRightToLeft);
        }

        small.Refresh(Places(), Count);
        Box? said = tray?.Rectangle();
        Say("the system says the icon is " + (said is Box spot ? "at " + spot.Left + "," + spot.Top + " to " + spot.Right + "," + spot.Bottom : "nowhere it can tell"));
        small.Toggle(said ?? Corner());
    }

    // Where the icon is when the system cannot say: the bottom right corner of the screen the main window is on.
    private Box Corner()
    {
        RectInt32 area = DisplayArea.GetFromWindowId(main.AppWindow.Id, DisplayAreaFallback.Nearest).OuterBounds;
        int right = area.X + area.Width;
        int bottom = area.Y + area.Height;
        return new Box(right - 48, bottom - 48, right - 16, bottom - 16);
    }

    // ---- what the core holds that the system shows ---------------------------------------------------------

    private IReadOnlyList<PlaceDestination> Places() =>
        [.. host.State.Nav.Destinations.Select(destination => new PlaceDestination(destination.Label, destination.Href, destination.Badge))];

    private void OnState(ViewState state)
    {
        changedSinceSnapshot = true;
        if (deferred is not null && state.Nav.Stage == Stage.Ready)
        {
            LaunchRequest waiting = deferred;
            deferred = null;
            Open(waiting);
        }

        ShowPlaces();
    }

    // The destinations the core holds now, where the system shows them: the small window by the icon, and the jump list. Nothing is done
    // when they are the ones last shown.
    private void ShowPlaces()
    {
        IReadOnlyList<PlaceDestination> places = Places();
        string key = string.Join("|", places.Select(place => place.Label + "\u001f" + place.Href + "\u001f" + place.Badge));
        if (key == placesKey)
        {
            return;
        }

        placesKey = key;
        small?.Refresh(places, Count);
        if (hasIdentity && Capabilities.Has(Offers, SystemFeature.JumpList) && places.Count > 0)
        {
            _ = UpdateJumpListAsync(places);
        }
    }

    private async Task UpdateJumpListAsync(IReadOnlyList<PlaceDestination> places)
    {
        bool saved = await JumpListSurface.UpdateAsync(places, host.Options.Place.JumpListLimit);
        Say("the jump list was " + (saved ? "saved with " + Math.Min(places.Count, host.Options.Place.JumpListLimit) + " entries" : "refused"));
    }

    /// <summary>Carries out what the app was asked from outside: a path of the app, a return from signing in, a new window, or only to come forward.</summary>
    /// <param name="request">What was asked.</param>
    public void Open(LaunchRequest request)
    {
        ArgumentNullException.ThrowIfNull(request);
        Say("asked from outside: " + (request.OpenHref is null ? string.Empty : "open " + request.OpenHref + " ") + (request.Return is null ? string.Empty : "a return on the app's scheme ") + (request.NewWindow ? "a new window" : string.Empty));

        // The app's destinations are known only once the core has shown them: what is asked before that waits for it.
        if ((request.OpenHref is not null || request.NewWindow) && host.State.Nav.Stage != Stage.Ready)
        {
            deferred = request;
            return;
        }

        if (request.Return is string address)
        {
            host.HandleReturn(address);
        }

        if (request.OpenHref is string href)
        {
            OpenAction action = OpeningRules.Decide(href, Places());
            switch (action.Kind)
            {
                case OpenKind.Select:
                    host.Send(Events.Select(action.Index));
                    main.HideSettings();
                    break;
                case OpenKind.Link:
                    host.Send(Events.OpenLink(action.Href));
                    main.HideSettings();
                    break;
                default:
                    break;
            }
        }

        if (request.NewWindow)
        {
            _ = OpenWindow();
        }

        if (!request.Background)
        {
            OpenMainWindow();
        }
    }

    private void Say(string line) => log.Add(DateTime.Now.ToString("HH:mm:ss.fff", System.Globalization.CultureInfo.InvariantCulture) + " " + line);

    /// <inheritdoc />
    public void Dispose()
    {
        timer.Stop();
        snapshotTimer.Stop();
        tray?.Dispose();
        badge.Dispose();
        IconHandle.Destroy(trayIcon);
        IconHandle.Destroy(appIcon);
        trayIcon = 0;
        appIcon = 0;
    }
}
