using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Controls;
using Windows.Foundation;

namespace Navree.Gallery;

/// <summary>Moves the real pointer over the window and presses its button, as a person at a mouse would.</summary>
internal static class Mouse
{
    private const uint LeftDown = 0x0002;
    private const uint LeftUp = 0x0004;

    [StructLayout(LayoutKind.Sequential)]
    private struct Point32
    {
        public int X;
        public int Y;
    }

    [DllImport("user32.dll")]
    private static extern bool SetCursorPos(int x, int y);

    [DllImport("user32.dll")]
    private static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);

    [DllImport("user32.dll")]
    private static extern bool ClientToScreen(IntPtr window, ref Point32 point);

    /// <summary>The place on the screen of a point of an element.</summary>
    /// <param name="window">The window.</param>
    /// <param name="element">The element.</param>
    /// <param name="at">The point, in the element's coordinates.</param>
    /// <returns>The screen's coordinates.</returns>
    internal static (int X, int Y) ScreenOf(MainWindow window, FrameworkElement element, Point at)
    {
        Point inWindow = element.TransformToVisual(null).TransformPoint(at);
        double scale = element.XamlRoot.RasterizationScale;
        Point32 origin = default;
        ClientToScreen(WinRT.Interop.WindowNative.GetWindowHandle(window), ref origin);
        return (origin.X + (int)Math.Round(inWindow.X * scale), origin.Y + (int)Math.Round(inWindow.Y * scale));
    }

    /// <summary>Moves the pointer to a place.</summary>
    internal static async Task MoveAsync((int X, int Y) to)
    {
        SetCursorPos(to.X, to.Y);
        await Task.Delay(120);
    }

    /// <summary>Presses and lets go of the left button where the pointer is.</summary>
    internal static async Task ClickAsync()
    {
        mouse_event(LeftDown, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(60);
        mouse_event(LeftUp, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(300);
    }

    /// <summary>Presses the left button, moves the pointer in steps, and lets go.</summary>
    internal static async Task DragAsync((int X, int Y) from, (int X, int Y) to)
    {
        await MoveAsync(from);
        mouse_event(LeftDown, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(120);
        const int steps = 8;
        for (int i = 1; i <= steps; i++)
        {
            SetCursorPos(from.X + ((to.X - from.X) * i / steps), from.Y + ((to.Y - from.Y) * i / steps));
            await Task.Delay(60);
        }

        await Task.Delay(120);
        mouse_event(LeftUp, 0, 0, 0, UIntPtr.Zero);
        await Task.Delay(400);
    }
}

/// <summary>Does with the pointer what a person does with a table: chooses a row, sorts, widens a column, moves a column.</summary>
internal static class PointerScenario
{
    internal static async Task RunAsync(MainWindow window)
    {
        Report.Line("== pointer");
        window.ThemeHost.RequestedTheme = ElementTheme.Light;
        ListDataTableModel<Customer> model = DemoData.Model();
        model.Reset(DemoData.Make(0, 300));
        model.Summary = "1–300 of 1,204";
        DataTable table = new() { Model = model, Presenter = new TextTablePresenter<Customer>(model), Culture = DemoData.Culture, Caption = "Customers", Margin = new Thickness(12) };
        List<string> events = [];
        table.ChoiceRequested += (_, e) => events.Add($"choice keys={string.Join(',', e.Keys)} on={e.On} all={e.All}");
        table.SortRequested += (_, e) => events.Add("sort " + e.Column.Key);
        table.RowInvoked += (_, e) => events.Add("invoked " + e.Key);
        table.LayoutChanged += (_, _) => events.Add("layout changed");
        window.StageHost.Child = table;
        await Waiting.LoadedAsync(table);
        await Task.Delay(1200);
        Keys.Foreground(window);
        await Task.Delay(300);

        string Columns() => string.Join(" ", table.Shown.Select(c => $"{c.Column.Key}:{c.Placed.Width:0}"));

        async Task Step(string said, Func<Task> act)
        {
            int before = events.Count;
            await act();
            await Task.Delay(300);
            string extra = events.Count > before ? "; events: " + string.Join(" / ", events.Skip(before)) : string.Empty;
            Report.Line($"[pointer] {said}: chosen {table.ChosenCount}, active {table.ActiveCell?.RowKey}/{table.ActiveCell?.ColumnKey}; columns {Columns()}{extra}");
        }

        DataTableRow? RowAt(int index) => table.RowsRepeater?.TryGetElement(index) as DataTableRow;
        DataTableHeaderCell Heading(string key) => table.HeaderRow!.Headers.First(h => table.Model!.Columns[h.ColumnIndex].Key == key);

        await Step("start", () => Task.CompletedTask);

        // The box of the third row.
        DataTableRow row2 = RowAt(2)!;
        DataTableCell? check = FindCheckCell(row2);
        if (check is not null)
        {
            (int X, int Y) at = Mouse.ScreenOf(window, check, new Point(check.ActualWidth / 2, check.ActualHeight / 2));
            await Step("click on the box of the third row", async () =>
            {
                await Mouse.MoveAsync(at);
                await Mouse.ClickAsync();
            });
        }

        // The name of the fifth row, with a box chosen: a plain click now chooses.
        DataTableCell nameCell = RowAt(4)!.Cells[0];
        (int X, int Y) nameAt = Mouse.ScreenOf(window, nameCell, new Point(40, nameCell.ActualHeight / 2));
        await Step("click on a name while a row is chosen", async () =>
        {
            await Mouse.MoveAsync(nameAt);
            await Mouse.ClickAsync();
        });

        // The heading of the city column.
        DataTableHeaderCell city = Heading("city");
        (int X, int Y) cityAt = Mouse.ScreenOf(window, city, new Point(30, city.ActualHeight / 2));
        await Step("click on a heading", async () =>
        {
            await Mouse.MoveAsync(cityAt);
            await Mouse.ClickAsync();
        });

        // The grip at the end of the city heading, dragged 60 to the right.
        (int X, int Y) gripAt = Mouse.ScreenOf(window, city, new Point(city.ActualWidth - 3, city.ActualHeight / 2));
        await Step("drag the end of the City heading 60 to the right", async () =>
        {
            await Mouse.DragAsync(gripAt, (gripAt.X + 60, gripAt.Y));
        });

        // The heading of the plan column, dragged left over the phone column.
        DataTableHeaderCell plan = Heading("plan");
        (int X, int Y) planAt = Mouse.ScreenOf(window, plan, new Point(30, plan.ActualHeight / 2));
        await Step("drag the Plan heading 160 to the left", async () =>
        {
            await Mouse.DragAsync(planAt, (planAt.X - 160, planAt.Y));
        });

        await Handshake.StepAsync("table-pointer-light", "after the pointer's changes");
    }

    private static DataTableCell? FindCheckCell(DataTableRow row)
    {
        // The box's cell is the first child of the frozen start layer.
        return row.FrozenStart?.Children.OfType<DataTableCell>().FirstOrDefault(c => c.Content is DataTableCheck);
    }
}
