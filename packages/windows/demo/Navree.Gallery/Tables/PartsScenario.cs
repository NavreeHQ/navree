using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Controls;
using Windows.Foundation;

namespace Navree.Gallery;

/// <summary>
/// Makes each part of the table on its own, gives it its template and measures it, and says which
/// part could not, so that a fault in one template is found by its name.
/// </summary>
internal static class PartsScenario
{
    internal static async Task RunAsync(MainWindow window)
    {
        Report.Line("== parts");
        try
        {
            ResourceDictionary brushes = new() { Source = new Uri("ms-appx:///Navree.Controls/Themes/DataTable.Resources.xaml") };
            Report.Line($"[parts] the brush dictionary loaded with {brushes.ThemeDictionaries.Count} theme dictionaries");
        }
        catch (Exception problem)
        {
            Report.Line("[parts] the brush dictionary did not load: " + problem);
        }

        (string Name, Func<Control> Make)[] parts =
        [
            ("DataTableCheck", () => new DataTableCheck()),
            ("DataTableGlyph", () => new DataTableGlyph { Glyph = "x" }),
            ("DataTableCell", () => new DataTableCell { Content = new TextBlock { Text = "cell" } }),
            ("DataTableHeaderCell", () => new DataTableHeaderCell()),
            ("ResizeGrip", () => new ResizeGrip()),
            ("DataTableRow", () => new DataTableRow()),
            ("DataTableListContent", () => new DataTableListContent()),
            ("DataTableBulkBar", () => new DataTableBulkBar()),
            ("DataTable", () => new DataTable()),
        ];
        foreach ((string name, Func<Control> make) in parts)
        {
            try
            {
                Control part = make();
                bool applied = part.ApplyTemplate();
                part.Measure(new Size(400, 100));
                Report.Line($"[parts] {name}: template {applied}, wants {part.DesiredSize.Width:0.#} by {part.DesiredSize.Height:0.#}");
                window.StageHost.Child = part;
                await Task.Delay(400);
                Report.Line($"[parts] {name}: on the stage, {part.ActualWidth:0.#} by {part.ActualHeight:0.#}");
            }
            catch (Exception problem)
            {
                Report.Line($"[parts] {name} failed: {problem}");
            }
        }

        window.StageHost.Child = null;
    }
}
