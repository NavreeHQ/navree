using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml;
using Navree.Shell;

namespace Navree.Demo;

/// <summary>The demo app: the client on recorded answers (or a live server), in one window.</summary>
public partial class App : Application
{
    private ShellHost? host;
    private ShellWindow? window;

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
        ShellOptions options = live is not null
            ? new ShellOptions { Origin = live, AppName = "Navree Demo", LoopbackHttp = live.StartsWith("http://", StringComparison.Ordinal) }
            : new ShellOptions
            {
                Origin = "https://charkha.example",
                AppName = "Navree Demo",
                Fetcher = answers is null ? null : new FixtureFetcher(answers),
                RecordedPages = answers is null ? null : address => FixtureFetcher.Page(answers, address),
                KeptFolder = Path.Combine(Path.GetTempPath(), "navree-demo-kept-" + Environment.ProcessId),
            };

        try
        {
            host = new ShellHost(options, DispatcherQueue.GetForCurrentThread());
            host.Problem += what => File.AppendAllText(Path.Combine(AppContext.BaseDirectory, "navree-demo.log"), "problem: " + what + Environment.NewLine);
            window = new ShellWindow(host);
            window.SetFlowDirection(Array.IndexOf(arguments, "--rtl") >= 0);
            window.Activate();
            host.Start();
        }
        catch (Exception problem)
        {
            File.AppendAllText(Path.Combine(AppContext.BaseDirectory, "navree-demo.log"), "start: " + problem + Environment.NewLine);
            throw;
        }

        if (Option(arguments, "--walk") is string folder)
        {
            _ = new Walker(host, window, folder).RunAsync();
        }
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
