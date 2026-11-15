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
            switch (name)
            {
                case "values":
                    await ValuesExperiment.RunAsync(window);
                    break;
                case "stick":
                    await StickExperiment.RunAsync(window, StickyMode.Compositor, rtl: false);
                    await StickExperiment.RunAsync(window, StickyMode.Compositor, rtl: true);
                    await StickExperiment.RunAsync(window, StickyMode.UiThread, rtl: false);
                    await StickExperiment.RunAsync(window, StickyMode.Static, rtl: false);
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
}
