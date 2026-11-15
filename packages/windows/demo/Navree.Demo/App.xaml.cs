using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Navree.Platform;
using Navree.Presence;
using Navree.Shell;

namespace Navree.Demo;

/// <summary>The demo app: the client on recorded answers (or a live server), in one window, with its place in Windows.</summary>
public partial class App : Application
{
    /// <summary>The app's own scheme, which a sign-in in the browser comes back on.</summary>
    public const string Scheme = "navree-demo";

    /// <summary>The key a second start finds the running app by.</summary>
    public const string InstanceKey = "navree-demo-main";

    private ShellHost? host;
    private ShellWindow? window;
    private SystemPlace? place;
    private DispatcherQueueTimer? probeTimer;

    /// <summary>Starts the app.</summary>
    public App()
    {
        InitializeComponent();
        string[] arguments = Environment.GetCommandLineArgs();

        // The demo is English, India unless it is asked for another locale: what the client writes itself (a day, a time) follows it.
        int at = Array.IndexOf(arguments, "--locale");
        System.Globalization.CultureInfo locale = Navree.Design.ClientCulture.Named(at >= 0 && at + 1 < arguments.Length ? arguments[at + 1] : "en-IN")
            ?? System.Globalization.CultureInfo.CurrentCulture;
        System.Globalization.CultureInfo.CurrentCulture = locale;
        System.Globalization.CultureInfo.CurrentUICulture = locale;
        System.Globalization.CultureInfo.DefaultThreadCurrentCulture = locale;
        System.Globalization.CultureInfo.DefaultThreadCurrentUICulture = locale;
        if (Array.IndexOf(arguments, "--dark") >= 0)
        {
            RequestedTheme = ApplicationTheme.Dark;
        }
        else if (Array.IndexOf(arguments, "--light") >= 0)
        {
            RequestedTheme = ApplicationTheme.Light;
        }

        UnhandledException += OnUnhandledException;
    }

    /// <inheritdoc />
    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        string[] arguments = Environment.GetCommandLineArgs();

        // The drawn kinds on their own, with no core behind them: each screen of a folder of pages, pictured.
        if (Option(arguments, "--kinds") is string kinds)
        {
            Window page = new() { Title = "Navree drawn kinds" };
            page.Activate();
            _ = new KindsWalker(page, kinds).RunAsync();
            return;
        }

        string? answers = Option(arguments, "--answers") ?? FixtureFetcher.Recorded();
        string? live = Option(arguments, "--server");
        bool testing = Option(arguments, "--walk") is not null || Option(arguments, "--system-walk") is not null || Option(arguments, "--probe") is not null || Option(arguments, "--run") is not null || Option(arguments, "--full") is not null || Option(arguments, "--checks") is not null || Option(arguments, "--moves") is not null || Option(arguments, "--popover") is not null || Option(arguments, "--hands") is not null;
        PlaceOptions placeOptions = new()
        {
            Scheme = Scheme,
            InstanceKey = InstanceKey,
            ChoicesFolder = testing ? Path.Combine(Path.GetTempPath(), "navree-demo-place-" + Environment.ProcessId) : null,
        };
        IFetcher? fetcher = Array.IndexOf(arguments, "--offline") >= 0
            ? new OfflineFetcher()
            : answers is null ? null : new FixtureFetcher(answers, Array.IndexOf(arguments, "--search") >= 0);
        ShellOptions options = live is not null
            ? new ShellOptions
            {
                Origin = live,
                AppName = "Navree Demo",
                LoopbackHttp = live.StartsWith("http://", StringComparison.Ordinal),
                KeptFolder = testing ? Path.Combine(Path.GetTempPath(), "navree-demo-kept-" + Environment.ProcessId) : null,
                Place = placeOptions,
            }
            : new ShellOptions
            {
                Origin = "https://charkha.example",
                AppName = "Navree Demo",
                Fetcher = fetcher is null ? null : new RecordingFetcher(fetcher),
                RecordedPages = answers is null ? null : address => FixtureFetcher.Page(answers, address),
                KeptFolder = testing ? Path.Combine(Path.GetTempPath(), "navree-demo-kept-" + Environment.ProcessId) : null,
                Place = placeOptions,
            };

        // A run that is to start with the icon in the notification area has the person's choice for it made beforehand.
        if (Array.IndexOf(arguments, "--tray") >= 0 && placeOptions.ChoicesFolder is string choicesFolder)
        {
            _ = new ChoicesFile(choicesFolder).Write(new Choices(true, true, true));
        }

        try
        {
            host = new ShellHost(options, DispatcherQueue.GetForCurrentThread());
            host.Problem += what => File.AppendAllText(Path.Combine(AppContext.BaseDirectory, "navree-demo.log"), "problem: " + what + Environment.NewLine);
            window = new ShellWindow(host);
            window.SetFlowDirection(Array.IndexOf(arguments, "--rtl") >= 0);
            place = new SystemPlace(host, window);
            host.Start();
            if (place.Start(Activation.Initial))
            {
                window.Activate();
            }
        }
        catch (Exception problem)
        {
            File.AppendAllText(Path.Combine(AppContext.BaseDirectory, "navree-demo.log"), "start: " + problem + Environment.NewLine);
            throw;
        }

        if (Option(arguments, "--walk") is string folder)
        {
            _ = new Walker(host, window, folder, place).RunAsync();
        }
        else if (Option(arguments, "--system-walk") is string systemFolder)
        {
            _ = new SystemWalker(host, window, place, systemFolder).RunAsync();
        }
        else if (Option(arguments, "--probe") is string probeFile)
        {
            Probe(probeFile);
        }
        else if (Option(arguments, "--full") is string fullFolder)
        {
            _ = new FullWalker(host, window, fullFolder, Option(arguments, "--name") ?? "app", place).RunAsync();
        }
        else if (Option(arguments, "--hands") is string handsFolder)
        {
            _ = new TableHands(host, window, handsFolder).RunAsync();
        }
        else if (Option(arguments, "--popover") is string popoverFolder)
        {
            _ = new PopoverCheck(host, window, popoverFolder).RunAsync();
        }
        else if (Option(arguments, "--moves") is string movesFolder)
        {
            _ = new BoardMoves(host, window, movesFolder).RunAsync();
        }
        else if (Option(arguments, "--checks") is string checksFolder)
        {
            _ = new ChecksWalker(host, window, checksFolder, Option(arguments, "--name") ?? "app", place).RunAsync();
        }
        else if (Option(arguments, "--run") == "press" && Option(arguments, "--out") is string pressed)
        {
            _ = new Pressing(host, window, pressed).RunAsync();
        }
    }

    // The app runs as it would for a person while something outside it (a second start, an address on its scheme) is tried on it; then it says what it was told.
    private void Probe(string file)
    {
        // Kept in a field: a timer nothing holds is collected, and never fires.
        probeTimer = DispatcherQueue.GetForCurrentThread().CreateTimer();
        probeTimer.Interval = TimeSpan.FromSeconds(45);
        probeTimer.IsRepeating = false;
        probeTimer.Tick += (_, _) =>
        {
            File.WriteAllLines(file, place?.Log ?? []);
            Finish(place);
        };
        probeTimer.Start();
    }

    /// <summary>Ends a run that nobody is watching: the windows are closed and the app exits; a run that is still there a while after is ended by force, so it never hangs a job.</summary>
    internal static void Finish(SystemPlace? place)
    {
        place?.LetExit();
        _ = Task.Delay(15000).ContinueWith(_ => Environment.Exit(0), TaskScheduler.Default);
        Current.Exit();
    }

    private static string? Option(string[] arguments, string name)
    {
        int at = Array.IndexOf(arguments, name);
        return at >= 0 && at + 1 < arguments.Length ? arguments[at + 1] : null;
    }

    // What stops the app is written beside it, so a run that nobody watched can be read afterwards.
    private static void OnUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs e)
    {
        File.AppendAllText(
            Path.Combine(AppContext.BaseDirectory, "navree-demo.log"),
            e.Exception + Environment.NewLine);
    }
}
