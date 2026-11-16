using Microsoft.UI.Xaml;
using Windows.Graphics;

namespace Navree.Gallery;

/// <summary>The gallery's window: a stage that shows one scenario at a time.</summary>
public sealed partial class MainWindow : Window
{
    private readonly GalleryArguments arguments;

    /// <summary>Makes the window and, when asked to, runs what it was asked to run.</summary>
    /// <param name="arguments">What the command line said.</param>
    public MainWindow(GalleryArguments arguments)
    {
        this.arguments = arguments;
        InitializeComponent();
        Root.Loaded += OnRootLoaded;
    }

    // The window is sized in effective pixels, so it is the same window on every display: 1180 by 780 is 2065 by 1365 at 175 percent. The least
    // size is in device pixels, so it is given again when the display scale changes.
    private void SizeWindow()
    {
        double scale = Root.XamlRoot?.RasterizationScale ?? 1.0;
        AppWindow.Resize(new SizeInt32(Navree.Design.WindowRules.Raw(1180, scale), Navree.Design.WindowRules.Raw(780, scale)));
        ApplyLeastSize();
        if (Root.XamlRoot is { } xamlRoot)
        {
            xamlRoot.Changed += (_, _) => ApplyLeastSize();
        }
    }

    private void ApplyLeastSize()
    {
        if (AppWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter presenter)
        {
            double scale = Root.XamlRoot?.RasterizationScale ?? 1.0;
            presenter.PreferredMinimumWidth = Navree.Design.WindowRules.Raw(640, scale);
            presenter.PreferredMinimumHeight = Navree.Design.WindowRules.Raw(420, scale);
        }
    }

    /// <summary>The stage a scenario puts its content on.</summary>
    internal Microsoft.UI.Xaml.Controls.Border StageHost => Stage;

    /// <summary>The element whose theme a scenario sets to show light and dark.</summary>
    internal FrameworkElement ThemeHost => Root;

    private async void OnRootLoaded(object sender, RoutedEventArgs e)
    {
        Report.Open(arguments.Out);
        Navree.Controls.DataTable.Watch(Report.Line);
        if (arguments.Run.Count == 0)
        {
            SizeWindow();
        }
        else
        {
            AppWindow.Resize(new SizeInt32(1000, 700));
        }
        if (arguments.Run.Count == 0)
        {
            // Started with nothing to run, the gallery is an app for a person.
            GalleryBrowser.Show(this);
            return;
        }

        try
        {
            await ScenarioRunner.RunAsync(this, arguments);
        }
        catch (Exception problem)
        {
            Report.Line("the run ended with an exception: " + problem);
        }
        finally
        {
            Report.Line("done");
            Application.Current.Exit();
        }
    }
}
