using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Navree.Platform;
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

    /// <summary>Starts the app.</summary>
    public App()
    {
        InitializeComponent();
        string[] arguments = Environment.GetCommandLineArgs();
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
        string? answers = Option(arguments, "--answers") ?? FixtureFetcher.Recorded();
        string? live = Option(arguments, "--server");
        bool testing = Option(arguments, "--walk") is not null || Option(arguments, "--system-walk") is not null || Option(arguments, "--probe") is not null;
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
            ? new ShellOptions { Origin = live, AppName = "Navree Demo", LoopbackHttp = live.StartsWith("http://", StringComparison.Ordinal), Place = placeOptions }
            : new ShellOptions
            {
                Origin = "https://charkha.example",
                AppName = "Navree Demo",
                Fetcher = fetcher,
                RecordedPages = answers is null ? null : address => FixtureFetcher.Page(answers, address),
                KeptFolder = testing ? Path.Combine(Path.GetTempPath(), "navree-demo-kept-" + Environment.ProcessId) : null,
                Place = placeOptions,
            };

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
    }

    // The app runs as it would for a person while something outside it (a second start, an address on its scheme) is tried on it; then it says what it was told.
    private void Probe(string file)
    {
        DispatcherQueueTimer timer = DispatcherQueue.GetForCurrentThread().CreateTimer();
        timer.Interval = TimeSpan.FromSeconds(45);
        timer.IsRepeating = false;
        timer.Tick += (_, _) =>
        {
            File.WriteAllLines(file, place?.Log ?? []);
            place?.LetExit();
            Exit();
        };
        timer.Start();
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
