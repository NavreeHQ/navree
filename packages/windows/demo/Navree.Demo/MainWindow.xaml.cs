using System.Text.Json;
using Microsoft.UI.Xaml;
using Navree.Core;
using Windows.Graphics;

namespace Navree.Demo;

/// <summary>The demo's window: the title bar, Mica behind it, and what the core answered at start.</summary>
public sealed partial class MainWindow : Window
{
    /// <summary>Makes the window and shows what the core said.</summary>
    public MainWindow()
    {
        InitializeComponent();

        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.Resize(new SizeInt32(1100, 760));

        CoreStatus.Text = AskTheCore() + Environment.NewLine + PackageIdentity();
        Root.Loaded += OnRootLoaded;
    }

    // Starts a core through the real library and reads its first answer.
    private static string AskTheCore()
    {
        try
        {
            using NavreeCore core = new("""{"origin":"https://shop.example","client":{"platform":"windows","form":"desktop","vocab":1,"text_percent":100}}"""u8);
            _ = core.Send("""{"t":"start"}"""u8);
            using JsonDocument nav = JsonDocument.Parse(core.View("""{"part":"nav"}"""u8));
            return "The core answered: " + nav.RootElement.GetProperty("stage").GetString();
        }
        catch (Exception problem)
        {
            return "The core did not answer: " + problem.GetType().Name + ": " + problem.Message;
        }
    }

    // Says whether the app runs with package identity: asking for the current package fails when it has none.
    private static string PackageIdentity()
    {
        try
        {
            return "Package identity: " + Windows.ApplicationModel.Package.Current.Id.FullName;
        }
        catch (Exception)
        {
            return "Package identity: none";
        }
    }

    // With --picture <file>, the window's content is drawn to a picture once it has settled, and the app ends.
    private async void OnRootLoaded(object sender, RoutedEventArgs e)
    {
        string? file = PictureArgument(Environment.GetCommandLineArgs());
        if (file is null)
        {
            return;
        }

        try
        {
            await Task.Delay(TimeSpan.FromSeconds(2));
            await WindowPicture.SaveAsync(Root, file);
        }
        catch (Exception problem)
        {
            File.WriteAllText(file + ".error.txt", problem.ToString());
        }
        finally
        {
            Application.Current.Exit();
        }
    }

    private static string? PictureArgument(string[] arguments)
    {
        int at = Array.IndexOf(arguments, "--picture");
        return at >= 0 && at + 1 < arguments.Length ? arguments[at + 1] : null;
    }
}
