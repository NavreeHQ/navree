using System.Diagnostics;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Microsoft.UI.Xaml.Controls;
using Navree.Controls;
using Windows.System;

namespace Navree.Gallery;

/// <summary>A table of a hundred thousand rows: how long it takes to show, whether rows stand where arithmetic says at the far end, and what choosing all costs.</summary>
internal static class HugeScenario
{
    private const int Count = 100_000;

    internal static async Task RunAsync(MainWindow window)
    {
        Report.Line("== rows100k");
        window.ThemeHost.RequestedTheme = ElementTheme.Light;
        ListDataTableModel<Customer> model = DemoData.Model();
        DataTable table = new() { Model = model, Presenter = new TextTablePresenter<Customer>(model), Culture = DemoData.Culture, Caption = "Customers", Margin = new Thickness(12) };
        window.StageHost.Child = table;
        await Waiting.LoadedAsync(table);
        await Task.Delay(1000);
        ScrollView scroll = table.Scroller!;
        double memoryBefore = Process.GetCurrentProcess().WorkingSet64 / 1048576.0;

        Stopwatch watch = Stopwatch.StartNew();
        List<Customer> customers = DemoData.Make(0, Count);
        long made = watch.ElapsedMilliseconds;
        watch.Restart();
        model.Reset(customers);
        model.Summary = "1–1,00,000 of 1,00,000";
        long handed = watch.ElapsedMilliseconds;
        while (table.RowsLayout.LastRealized.Count == 0 && watch.ElapsedMilliseconds < 20000)
        {
            await Task.Delay(10);
        }

        long shown = watch.ElapsedMilliseconds;
        await Task.Delay(1000);
        double memoryAfter = Process.GetCurrentProcess().WorkingSet64 / 1048576.0;
        Report.Line($"[rows100k] {Count:N0} rows made by the gallery in {made} ms; handed to the table in {handed} ms; the first rows were on screen {shown} ms after; extent {scroll.ExtentHeight:N0}; rows made {table.RowsLayout.LastRealized.Count}; memory {memoryBefore:0} MB before, {memoryAfter:0} MB after");

        // Where rows stand at ten places along the scroll bar.
        double rowHeight = table.RowHeight;
        double worst = 0;
        foreach (double fraction in new[] { 0.0, 0.1, 0.25, 0.4, 0.5, 0.63, 0.77, 0.9, 0.99, 1.0 })
        {
            double offset = fraction * Math.Max(0, scroll.ExtentHeight - scroll.ViewportHeight);
            scroll.ScrollTo(0, offset, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
            await Task.Delay(700);
            int index = (int)(scroll.VerticalOffset / rowHeight);
            if (table.RowsRepeater?.TryGetElement(index) is { } element)
            {
                double top = element.TransformToVisual(scroll).TransformPoint(new Windows.Foundation.Point(0, 0)).Y;
                double expected = (index * rowHeight) - scroll.VerticalOffset;
                worst = Math.Max(worst, Math.Abs(top - expected));
                Report.Line($"[rows100k] at {fraction:P0}: offset {scroll.VerticalOffset:N0}, row {index:N0} stands {top:0.##} from the top of the view, arithmetic says {expected:0.##}");
            }
            else
            {
                Report.Line($"[rows100k] at {fraction:P0}: row {index:N0} was not made");
            }
        }

        Report.Line($"[rows100k] the largest difference between where a row stood and where arithmetic put it: {worst:0.##}");

        // Ctrl+End by the keyboard, timed to the last row focused.
        scroll.ScrollTo(0, 0, new ScrollingScrollOptions(ScrollingAnimationMode.Disabled, ScrollingSnapPointsMode.Ignore));
        await Task.Delay(500);
        Keys.Foreground(window);
        table.RestoreFocus();
        await Task.Delay(500);
        string last = model.GetRow(Count - 1).Key!;
        watch.Restart();
        await Keys.PressAsync(VirtualKey.End, VirtualKey.Control);
        while (table.ActiveCell?.RowKey != last && watch.ElapsedMilliseconds < 10000)
        {
            await Task.Delay(10);
        }

        long ctrlEnd = watch.ElapsedMilliseconds;
        await Task.Delay(500);
        Report.Line($"[rows100k] Ctrl+End: the last row was the active cell after {ctrlEnd} ms; focus is on {FocusedWords(table)}; the view is at {scroll.VerticalOffset:N0} of {scroll.ExtentHeight - scroll.ViewportHeight:N0}");

        // Choosing all of them.
        List<string> events = [];
        table.ChoiceRequested += (_, e) => events.Add($"choice all={e.All} on={e.On}");
        double memoryBeforeChoice = Process.GetCurrentProcess().WorkingSet64 / 1048576.0;
        watch.Restart();
        table.ChooseAllHeld();
        long chose = watch.ElapsedMilliseconds;
        Report.Line($"[rows100k] choosing all took {chose} ms: {table.ChosenCount:N0} chosen, {events.Count} event(s) {string.Join(",", events)}; memory {memoryBeforeChoice:0} MB before and {Process.GetCurrentProcess().WorkingSet64 / 1048576.0:0} MB after; said: {table.LastAnnouncement}");
        table.ClearChoice();

        // A row far away, asked for by assistive technology and made on request.
        if (FrameworkElementAutomationPeer.CreatePeerForElement(table) is DataTableAutomationPeer peer)
        {
            string key = model.GetRow(50_000).Key!;
            IRawElementProviderSimple? found = ((IItemContainerProvider)peer).FindItemByProperty(null!, AutomationElementIdentifiers.AutomationIdProperty, key);
            DataTableRowStandInAutomationPeer? standIn = peer.RowStandIn(50_000);
            Report.Line($"[rows100k] asked for {key} by its id: found {found is not null}; its stand-in is called '{standIn?.GetName()}' and has the id {standIn?.GetAutomationId()}; it is made now: {table.RowsRepeater?.TryGetElement(50_000) is not null}");
            standIn?.Realize();
            await Task.Delay(500);
            Report.Line($"[rows100k] after Realize the row is {(table.RowsRepeater?.TryGetElement(50_000) is not null ? "made" : "not made")}");
        }

        await Handshake.StepAsync("table-rows100k-light", "a table of a hundred thousand rows, at the last row");
    }

    private static string FocusedWords(DataTable table) =>
        Microsoft.UI.Xaml.Input.FocusManager.GetFocusedElement(table.XamlRoot) switch
        {
            DataTableCell cell => $"a cell of row {cell.Row?.Index:N0}",
            null => "nothing",
            { } other => other.GetType().Name,
        };
}
