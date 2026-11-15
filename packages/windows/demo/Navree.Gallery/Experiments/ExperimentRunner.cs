using Navree.Controls;

namespace Navree.Gallery;

/// <summary>Runs the scenarios the command line asked for, one after the other.</summary>
internal static class ExperimentRunner
{
    internal static async Task RunAsync(MainWindow window, GalleryArguments arguments)
    {
        Handshake.Open(arguments.Out);
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
                await MirrorExperiment.RunAsync(window);
                break;
            case "parts":
                await PartsScenario.RunAsync(window);
                break;
            case "table":
                await TableScenarios.RunAsync(window, TableScenarios.All);
                break;
            case "translation":
                await TranslationExperiment.RunAsync(window);
                break;
            case "values":
                await ValuesExperiment.RunAsync(window);
                break;
            case "stick":
                await StickExperiment.RunAsync(window, StickyMode.Compositor, rtl: false);
                await StickExperiment.RunAsync(window, StickyMode.Compositor, rtl: true);
                await StickExperiment.RunAsync(window, StickyMode.UiThread, rtl: false);
                break;
            case "anchor":
                await AnchorExperiment.RunAsync(window);
                break;
            default:
                Report.Line("unknown scenario: " + name);
                break;
        }
    }
}
