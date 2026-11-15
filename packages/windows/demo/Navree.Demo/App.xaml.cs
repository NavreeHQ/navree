using Microsoft.UI.Xaml;

namespace Navree.Demo;

/// <summary>The demo app.</summary>
public partial class App : Application
{
    private MainWindow? window;

    /// <summary>Starts the app.</summary>
    public App()
    {
        InitializeComponent();
        UnhandledException += OnUnhandledException;
    }

    /// <inheritdoc />
    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        window = new MainWindow();
        window.Activate();
    }

    // What stops the app is written beside it, so a run that nobody watched can be read afterwards.
    private static void OnUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs e)
    {
        File.AppendAllText(
            Path.Combine(AppContext.BaseDirectory, "navree-demo.log"),
            e.Exception + Environment.NewLine);
    }
}
