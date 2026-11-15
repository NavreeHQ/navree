using Navree.Controls;

namespace Navree.Gallery;

/// <summary>Runs the scenarios the command line asked for, one after the other.</summary>
internal static class ScenarioRunner
{
    internal static async Task RunAsync(MainWindow window, GalleryArguments arguments)
    {
        Handshake.Open(arguments.Out);
        try
        {
            Windows.UI.ViewManagement.AccessibilitySettings access = new();
            Report.Line($"[settings] text scale {new Windows.UI.ViewManagement.UISettings().TextScaleFactor}, high contrast {access.HighContrast} ({access.HighContrastScheme})");
        }
        catch (Exception problem)
        {
            Report.Line("[settings] could not be read: " + problem.Message);
        }

        foreach (string name in arguments.Run)
        {
            try
            {
                await RunOneAsync(window, name);
            }
            catch (Exception problem)
            {
                Report.Line($"[{name}] ended with an exception: {problem}");
            }
        }
    }

    private static async Task RunOneAsync(MainWindow window, string name)
    {
        switch (name)
        {
            case "pointer":
                await PointerScenario.RunAsync(window);
                break;
            case "mirror":
                await MirrorScenario.RunAsync(window);
                break;
            case "parts":
                await PartsScenario.RunAsync(window);
                break;
            case "tabletext":
                await TableScenarios.RunAsync(window, ["standard", "chosen", "narrow", "sorted", "rtl"], lightOnly: true);
                break;
            case "rows100k":
                await HugeScenario.RunAsync(window);
                break;
            case "table":
                await TableScenarios.RunAsync(window, TableScenarios.All);
                break;
            case "translation":
                await TranslationScenario.RunAsync(window);
                break;
            case "values":
                await ScrollNumbersScenario.RunAsync(window);
                break;
            case "stick":
                await StickScenario.RunAsync(window, StickyMode.Compositor, rtl: false);
                await StickScenario.RunAsync(window, StickyMode.Compositor, rtl: true);
                await StickScenario.RunAsync(window, StickyMode.UiThread, rtl: false);
                break;
            case "anchor":
                await AnchorScenario.RunAsync(window);
                break;
            default:
                Report.Line("unknown scenario: " + name);
                break;
        }
    }
}
