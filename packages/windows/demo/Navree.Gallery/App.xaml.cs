using Microsoft.UI.Xaml;

namespace Navree.Gallery;

/// <summary>The gallery app.</summary>
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
        window = new MainWindow(GalleryArguments.Read(Environment.GetCommandLineArgs()));
        window.Activate();
    }

    // What stops the app is written beside it, so a run that nobody watched can be read afterwards.
    private static void OnUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs e)
    {
        string text = e.Exception + Environment.NewLine;
        File.AppendAllText(Path.Combine(AppContext.BaseDirectory, "navree-gallery.log"), text);
        Report.Line("unhandled exception: " + e.Exception.GetType().Name + ": " + e.Exception.Message);
    }
}
