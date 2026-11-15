using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Navree.Controls;
using Navree.Runtime;

namespace Navree.Shell;

/// <summary>
/// What the person has on a page when an answer makes it be drawn again: where the focus is (and the caret and selection of a text box, and the
/// active cell of a table), carried to the same node of the new page by the node's id. The core's own request to move the focus is carried out by
/// the same means. A page is built again from the tree; this is what keeps it from being a different page to the person using it.
/// </summary>
internal sealed class PageKeeper
{
    /// <summary>What was where the focus was.</summary>
    /// <param name="Node">The id of the node that held the focus, or of the table the active cell was in.</param>
    /// <param name="Start">The caret of a text box, or -1.</param>
    /// <param name="Length">The selection of a text box.</param>
    /// <param name="Row">The active row of a table, by key.</param>
    /// <param name="Column">The active column of a table, by key.</param>
    internal sealed record Held(string Node, int Start, int Length, string? Row, string? Column);

    private readonly Dictionary<long, long> served = [];

    /// <summary>Gives a drawn node the id the tree gave it, so it can be found again after the page is drawn again.</summary>
    /// <param name="element">What was drawn.</param>
    /// <param name="id">The node's id.</param>
    internal static void Mark(FrameworkElement element, string id)
    {
        if (string.IsNullOrEmpty(AutomationProperties.GetAutomationId(element)))
        {
            AutomationProperties.SetAutomationId(element, id);
        }
    }

    /// <summary>Reads what has the focus on a page, before the page is replaced.</summary>
    /// <param name="page">The page that is on the screen.</param>
    /// <returns>What to put back, or none when nothing on the page has the focus.</returns>
    internal static Held? Capture(FrameworkElement? page)
    {
        if (page?.XamlRoot is null || FocusManager.GetFocusedElement(page.XamlRoot) is not DependencyObject focused || !Inside(page, focused))
        {
            return null;
        }

        string? id = null;
        string? row = null;
        string? column = null;
        for (DependencyObject? up = focused; up is not null && !ReferenceEquals(up, page); up = VisualTreeHelper.GetParent(up))
        {
            if (up is DataTable table && table.ActiveCell is { } cell && AutomationProperties.GetAutomationId(table) is { Length: > 0 } tableId)
            {
                // A table's active cell is carried by the table's id, with the row and column it was on.
                id = tableId;
                row = cell.RowKey;
                column = cell.ColumnKey;
                break;
            }

            if (id is null && up is FrameworkElement element && AutomationProperties.GetAutomationId(element) is { Length: > 0 } found)
            {
                // The nearest part with an id is the one the focus is in.
                id = found;
            }
        }

        if (id is null)
        {
            return null;
        }

        return focused is TextBox box
            ? new Held(id, box.SelectionStart, box.SelectionLength, null, null)
            : new Held(id, -1, 0, row, column);
    }

    /// <summary>Puts back what was held, in the new page, once it has laid itself out; tried again for a short while, since a table realises its rows after it loads.</summary>
    /// <param name="page">The new page.</param>
    /// <param name="held">What was held.</param>
    internal static void Restore(FrameworkElement page, Held held)
    {
        int attempts = 0;
        void Try()
        {
            attempts++;
            if (page.XamlRoot is null || !Give(page, held.Node, held))
            {
                if (attempts < 12 && page.XamlRoot is not null)
                {
                    page.DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, async () =>
                    {
                        await Task.Delay(80);
                        Try();
                    });
                }
            }
        }

        if (page.IsLoaded)
        {
            page.DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, Try);
        }
        else
        {
            RoutedEventHandler? loaded = null;
            loaded = (_, _) =>
            {
                page.Loaded -= loaded;
                page.DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, Try);
            };
            page.Loaded += loaded;
        }
    }

    /// <summary>Carries out the core's request to move the focus, once for each serial, and answers it.</summary>
    /// <param name="page">The page that is on the screen.</param>
    /// <param name="screen">The screen's state.</param>
    /// <param name="send">Tells the core.</param>
    internal void Serve(FrameworkElement page, ScreenView screen, Action<string> send)
    {
        if (screen.Focus is not { } request || served.TryGetValue(screen.Id, out long last) && last >= request.Serial)
        {
            return;
        }

        served[screen.Id] = request.Serial;
        int attempts = 0;
        void Try()
        {
            attempts++;
            if (Give(page, request.Target, null))
            {
                send(Events.Focused(screen.Id, request.Serial));
            }
            else if (attempts < 12 && page.XamlRoot is not null)
            {
                page.DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, async () =>
                {
                    await Task.Delay(80);
                    Try();
                });
            }
        }

        page.DispatcherQueue.TryEnqueue(Microsoft.UI.Dispatching.DispatcherQueuePriority.Low, Try);
    }

    // Gives the focus to the node of the page with this id; false until it is there to be given it.
    private static bool Give(FrameworkElement page, string id, Held? held)
    {
        FrameworkElement? target = Find(page, id);
        if (target is null)
        {
            return false;
        }

        bool done;
        if (target is DataTable table && held?.Row is { } row)
        {
            done = table.FocusRow(row, held.Column);
        }
        else if (target is Control control && control.IsTabStop)
        {
            done = control.Focus(FocusState.Programmatic);
        }
        else
        {
            done = FocusManager.FindFirstFocusableElement(target) is { } first && first is Control inner && inner.Focus(FocusState.Programmatic);
        }

        if (done)
        {
            target.StartBringIntoView();
            if (held is { Start: >= 0 } && FocusManager.GetFocusedElement(page.XamlRoot) is TextBox box)
            {
                box.Select(Math.Min(held.Start, box.Text.Length), Math.Min(held.Length, Math.Max(0, box.Text.Length - held.Start)));
            }
        }

        return done;
    }

    private static FrameworkElement? Find(DependencyObject at, string id)
    {
        if (at is FrameworkElement element && AutomationProperties.GetAutomationId(element) == id)
        {
            return element;
        }

        int count = VisualTreeHelper.GetChildrenCount(at);
        for (int child = 0; child < count; child++)
        {
            if (Find(VisualTreeHelper.GetChild(at, child), id) is { } found)
            {
                return found;
            }
        }

        return null;
    }

    private static bool Inside(DependencyObject page, DependencyObject element)
    {
        for (DependencyObject? up = element; up is not null; up = VisualTreeHelper.GetParent(up))
        {
            if (ReferenceEquals(up, page))
            {
                return true;
            }
        }

        return false;
    }
}
