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
        AppWindow.Resize(new SizeInt32(1000, 700));
        Root.Loaded += OnRootLoaded;
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
            Stage.Child = new Microsoft.UI.Xaml.Controls.TextBlock
            {
                Text = "The gallery runs scenarios: start it with --run stick,anchor --out <folder>.",
                Margin = new Thickness(24),
            };
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
