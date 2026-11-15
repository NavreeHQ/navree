using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Navree.Controls;
using Navree.Design;
using Navree.Design.Pages;
using Navree.Design.Tables;
using Navree.Protocol;
using Navree.Runtime;
using DesignRole = Navree.Design.Tables.ColumnRole;

namespace Navree.Shell;

// The table of the protocol drawn with the table of our own: columns, frozen cells, choosing, rows by cursor.
// The table's query form is the toolbar above it: a search box and a Filter button that opens the other fields.
internal sealed partial class NodeRenderer
{
    private const double TableHeight = 640;
    private const double TableLeastHeight = 320;

    // The flyouts of Filter buttons that are open, by screen and form: a screen is drawn again when its table's rows arrive, which
    // takes the flyout away with the button it was on, and a person ticking a second box would find it gone.
    private static readonly HashSet<(long Screen, string Form)> FiltersOpen = [];

    private UIElement DrawTable(Models.Table table)
    {
        string? id = table.Id;
        long screen = context.Screen.Id;
        string? inOverlay = currentOverlay;
        void Submit(Models.Button button, string? form)
        {
            if (button.Action is not null)
            {
                context.Send(Events.Act(screen, button.Action, form, inOverlay, null, null, button.Ask, button.Id));
            }
        }

        // What the handlers read is the table as it stands when they are used: an answer that brings the table up to date puts the new rows here.
        TableHold hold = new(table);
        ProtocolTableModel model = new(table, context.Screen);
        ProtocolTablePresenter presenter = new(table, node => RenderLoose(node), MenuFor, nodes => Column(nodes), Submit);
        DataTable view = new()
        {
            Model = model,
            Presenter = presenter,
            Caption = table.Caption,
            Culture = CultureInfo.CurrentCulture,
            Height = TableHeight,
            VerticalAlignment = VerticalAlignment.Top,
        };

        if (id is not null && context.Layouts is { } layouts)
        {
            // A person's widths and order of the columns are kept for each table of each screen, from one launch to the next.
            string address = context.Screen.Url.Split('?', 2)[0];
            view.LayoutKey = address + "#" + id;
            view.LayoutStore = layouts;
        }

        if (table.Empty is not null)
        {
            view.EmptyContent = Render(table.Empty);
        }

        view.ApplyChoice(hold.ByKey.Values.Where(row => row.Selected).Select(row => row.Id!), model.AcrossOn);
        view.ChoiceRequested += (_, e) =>
        {
            if (id is null)
            {
                return;
            }

            if (e.Across)
            {
                context.Send(Events.Choose(screen, id, null, true, across: true));
            }
            else if (e.All)
            {
                context.Send(Events.Choose(screen, id, null, e.On));
            }
            else
            {
                foreach (string key in e.Keys)
                {
                    context.Send(Events.Choose(screen, id, key, e.On));
                }
            }
        };
        view.SortRequested += (_, e) =>
        {
            if (e.Column.Sort is not { } sort)
            {
                return;
            }

            if (id is not null)
            {
                context.Send(Events.RegionGo(screen, id, sort.Address));
            }
            else
            {
                // A table with no id goes to the sorted address as the page itself.
                Act(new Models.ActionNavigate { Href = sort.Address, Present = Models.Present.Replace }, null, null, null, inOverlay);
            }
        };
        view.MoreRequested += (_, e) =>
        {
            if (id is not null)
            {
                context.Send(Events.More(screen, id, before: e.Direction == MoreDirection.Before));
            }
        };
        view.RetryRequested += (_, _) =>
        {
            if (id is not null)
            {
                context.Send(Events.More(screen, id));
            }
        };
        view.RowInvoked += (_, e) =>
        {
            if (!hold.ByKey.TryGetValue(e.Key, out Models.TableRow? row))
            {
                return;
            }

            if (row.Action is not null)
            {
                Act(row.Action, null, row.Id, null, inOverlay);
            }
            else if (ProtocolTableModel.PrimaryOf(hold.Table, row) is { } primary)
            {
                Submit(primary.Button, primary.Form);
            }
        };
        view.DetailToggled += (_, e) => context.Send(Events.Overlay(screen, e.Key, e.Open));
        view.EditRequested += (_, e) =>
        {
            if (hold.ByKey.TryGetValue(e.Key, out Models.TableRow? row) && ProtocolTableModel.EditOf(row) is { } edit)
            {
                Submit(edit.Button, edit.Form);
            }
        };
        bool fitting = false;
        view.Loaded += (_, _) =>
        {
            if (!fitting)
            {
                fitting = true;
                FitHeight(view);
            }
        };

        // A caption that says what the screen's own title says is not drawn again: it stays the table's name for a screen reader.
        StackPanel panel = new() { Spacing = 8 };
        if (table.Caption is not null && !SaysTheTitle(table.Caption))
        {
            panel.Children.Add(Look.Label(table.Caption, "BodyStrongTextBlockStyle"));
        }

        int toolbarFrom = current!.Children.Count;
        if (table.Query is not null && TableToolbar(table.Query) is { } toolbar)
        {
            view.Toolbar = toolbar;
        }

        // The fields of the bar are the records drawn for it, in the order they were drawn: what an answer brings up to date is found by place.
        List<Drawn> toolbarRecords = [.. current.Children.Skip(toolbarFrom)];

        panel.Children.Add(view);

        // An answer brings the table up to date where it stands: its model takes the new rows and says what changed, so the table keeps its place, its
        // active cell, its widths and what is chosen; its search box and filters are fields, which stand and show what the screen holds for them.
        Drawn me = current!;
        me.Update = now =>
        {
            if (now is not Models.Table next || me.Node is not Models.Table previous || !TableStands(previous, next))
            {
                return false;
            }

            if (!PageRules.SameButFieldValues(QueryNode(previous), QueryNode(next)) && !UpdateToolbar(toolbarRecords, next))
            {
                return false;
            }

            bool chosenBefore = hold.Table.Rows.Where(row => row.Selected).Select(row => row.Id).SequenceEqual(next.Rows.Where(row => row.Selected).Select(row => row.Id));
            hold.Put(next);
            presenter.Table = next;
            model.Apply(next, context.Screen);
            if (!chosenBefore || model.AcrossOn != view.AcrossChosen)
            {
                view.ApplyChoice(hold.ByKey.Values.Where(row => row.Selected).Select(row => row.Id!), model.AcrossOn);
            }

            return true;
        };
        return panel;
    }

    private static Models.Node QueryNode(Models.Table table) => table.Query is null ? new Models.Spacer() : table.Query;

    // What a table that stands holds the same: what it is, how it is named, what it says when empty, and whether rows can be chosen. Its rows, its
    // columns (the sort a heading shows), its cursors and its summary are what an answer changes.
    private static bool TableStands(Models.Table previous, Models.Table next) =>
        previous.Id == next.Id
        && previous.Caption == next.Caption
        && (previous.Empty is null) == (next.Empty is null)
        && (previous.Empty is null || PageRules.Same(previous.Empty, next.Empty!))
        && (previous.Select is null) == (next.Select is null)
        && (previous.Query is null) == (next.Query is null)
        && previous.Columns.Count == next.Columns.Count
        && previous.Columns.Select(column => column.Key ?? column.Label).SequenceEqual(next.Columns.Select(column => column.Key ?? column.Label));

    // The fields of the query form are drawn in the bar above the table and stand: each is the same control while only its starting value changed, and
    // what the screen holds for it is shown by its handle. A query with other fields, or a field that changed otherwise, is a table drawn again.
    private static bool UpdateToolbar(List<Drawn> records, Models.Table next)
    {
        List<Models.Node> now = QueryFields(next.Query!);
        if (records.Count != now.Count)
        {
            return false;
        }

        for (int at = 0; at < now.Count; at++)
        {
            if (!PageRules.SameButFieldValues(records[at].Node, now[at]))
            {
                return false;
            }
        }

        for (int at = 0; at < now.Count; at++)
        {
            records[at].Node = now[at];
        }

        return true;
    }

    // The nodes the query form is drawn from, in the order they are drawn: the search field, then the rest.
    private static List<Models.Node> QueryFields(Models.Form query)
    {
        Models.Field? search = query.Children.OfType<Models.Field>().FirstOrDefault(field => field.Input is Models.TextInput { Kind: Models.TextKind.Search });
        return search is null ? [.. query.Children] : [search, .. query.Children.Where(child => !ReferenceEquals(child, search))];
    }

    /// <summary>The table a drawn table of the protocol stands for now, with its rows by key.</summary>
    private sealed class TableHold
    {
        internal TableHold(Models.Table table) => Put(table);

        internal Models.Table Table { get; private set; } = null!;

        internal Dictionary<string, Models.TableRow> ByKey { get; private set; } = [];

        internal void Put(Models.Table table)
        {
            Table = table;
            ByKey = new Dictionary<string, Models.TableRow>(StringComparer.Ordinal);
            foreach (Models.TableRow row in table.Rows.Where(row => row.Id is not null))
            {
                // Two rows with one id: the first is the one the id names.
                _ = ByKey.TryAdd(row.Id!, row);
            }
        }
    }

    private bool SaysTheTitle(string caption) =>
        context.Screen.Screen?.Title is { } title && string.Equals(caption.Trim(), title.Trim(), StringComparison.CurrentCultureIgnoreCase);

    // The table takes the room the page has under it, so it is not a short box in a tall window and not taller than the window.
    private static void FitHeight(DataTable view)
    {
        ScrollViewer? page = null;
        DependencyObject? up = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(view);
        while (up is not null)
        {
            if (up is ScrollViewer found)
            {
                page = found;
                break;
            }

            up = Microsoft.UI.Xaml.Media.VisualTreeHelper.GetParent(up);
        }

        if (page is null)
        {
            return;
        }

        ScrollViewer host = page;

        void Fit()
        {
            if (host.ViewportHeight <= 0)
            {
                return;
            }

            double top = view.TransformToVisual(host).TransformPoint(new Windows.Foundation.Point(0, 0)).Y + host.VerticalOffset;
            double room = host.ViewportHeight - top - 24;
            view.Height = Math.Max(TableLeastHeight, room);
        }

        host.SizeChanged += (_, _) => Fit();
        Fit();
        view.DispatcherQueue.TryEnqueue(() => Fit());
    }

    // The query form, as a bar: the search field in line, and the other fields in a flyout behind a Filter button.
    private UIElement? TableToolbar(Models.Form query)
    {
        string form = Tree.FormId(query);
        Models.Field? search = query.Children.OfType<Models.Field>().FirstOrDefault(field => field.Input is Models.TextInput { Kind: Models.TextKind.Search });
        List<Models.Node> others = query.Children.Where(child => !ReferenceEquals(child, search)).ToList();
        if (search is null && others.Count == 0)
        {
            return null;
        }

        Grid bar = new() { ColumnSpacing = 8, Padding = new Thickness(8, 8, 8, 8) };
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        bar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        if (search is not null)
        {
            UIElement drawn = InForm(form, () => Render(search));
            ClearHeader(drawn);
            Grid.SetColumn((FrameworkElement)drawn, 0);
            bar.Children.Add(drawn);
        }

        if (others.Count > 0)
        {
            StackPanel fields = InForm(form, () => (StackPanel)Column(others, 16));
            fields.Width = 320;
            fields.Padding = new Thickness(4);
            Flyout flyout = new() { Content = new ScrollViewer { Content = fields, MaxHeight = 480 }, Placement = FlyoutPlacementMode.BottomEdgeAlignedRight };
            Button filter = new() { Content = "Filter", VerticalAlignment = VerticalAlignment.Bottom, Flyout = flyout };
            (long, string) key = (context.Screen.Id, form);
            flyout.Opened += (_, _) => FiltersOpen.Add(key);

            // Closed by the person, the button is still on the screen when the queue gets to this; closed by the screen being drawn again, it is not.
            flyout.Closed += (_, _) => filter.DispatcherQueue.TryEnqueue(() =>
            {
                if (filter.XamlRoot is not null)
                {
                    FiltersOpen.Remove(key);
                }
            });
            if (FiltersOpen.Contains(key))
            {
                RoutedEventHandler? reopen = null;
                reopen = (_, _) =>
                {
                    filter.Loaded -= reopen;
                    filter.DispatcherQueue.TryEnqueue(() => flyout.ShowAt(filter));
                };
                filter.Loaded += reopen;
            }

            Grid.SetColumn(filter, 1);
            bar.Children.Add(filter);
        }

        return bar;
    }

    // A search box in a bar has its placeholder and no label over it.
    private static void ClearHeader(UIElement drawn)
    {
        if (drawn is Panel panel)
        {
            foreach (UIElement child in panel.Children)
            {
                if (child is TextBox box)
                {
                    box.Header = null;
                }
            }
        }
    }
}

/// <summary>
/// A protocol table as the table control reads it: the rows the core sent, with the state of its cursor. When an answer brings the table up to
/// date the model takes the new rows and tells the table what changed (rows joined at an end, rows drawn again, every row new, other columns, another
/// cursor), so the table that is on the screen stays and keeps its place.
/// </summary>
internal sealed class ProtocolTableModel : IDataTableModel
{
    private const int Sample = 40;

    private Models.Table table;
    private ScreenView applied;
    private DataTableColumn[] columns;
    private readonly Dictionary<string, int> places = new(StringComparer.Ordinal);
    private List<Models.TableRow> rows;
    private ListState? state;
    private EventHandler<DataTableModelChangedEventArgs>? changed;

    internal ProtocolTableModel(Models.Table protocol, ScreenView screen)
    {
        table = protocol;
        applied = screen;
        state = StateOf(protocol, screen);
        columns = protocol.Columns.Select((column, index) => ColumnOf(column, index, protocol.Rows)).ToArray();
        rows = [.. protocol.Rows];
        Place();
    }

    private static ListState? StateOf(Models.Table protocol, ScreenView screen) =>
        protocol.Id is { } id && screen.Lists.TryGetValue(id, out ListState? held) ? held : null;

    private void Place()
    {
        places.Clear();
        for (int i = 0; i < rows.Count; i++)
        {
            if (rows[i].Id is { } key)
            {
                places[key] = i;
            }
        }
    }

    /// <summary>The table the model holds now.</summary>
    internal Models.Table Table => table;

    /// <inheritdoc />
    public event EventHandler<DataTableModelChangedEventArgs>? Changed
    {
        add => changed += value;
        remove => changed -= value;
    }

    /// <summary>
    /// Takes the table that stands there now and says what changed: the columns, the rows (joined at the end or the start, drawn again where they
    /// stand, or every one new), the cursors and the summary.
    /// </summary>
    /// <param name="next">The table.</param>
    /// <param name="screen">The screen it is on.</param>
    internal void Apply(Models.Table next, ScreenView screen)
    {
        DataTableCursor later = Later;
        DataTableCursor earlier = Earlier;
        string? summary = Summary;
        DataTableColumn[] nextColumns = next.Columns.Select((column, index) => ColumnOf(column, index, next.Rows)).ToArray();
        bool columnsChanged = !columns.AsSpan().SequenceEqual(nextColumns);
        List<Models.TableRow> before = rows;
        (List<Models.TableRow> after, DataTableModelChangedEventArgs[] events) = Diff(before, [.. next.Rows], new ScreenFacts(applied, fields: false), new ScreenFacts(screen, fields: false));
        applied = screen;
        table = next;
        state = StateOf(next, screen);
        columns = nextColumns;
        rows = after;
        Place();
        if (columnsChanged)
        {
            changed?.Invoke(this, new DataTableModelChangedEventArgs(DataTableChangeKind.Columns));
        }

        foreach (DataTableModelChangedEventArgs one in events)
        {
            changed?.Invoke(this, one);
        }

        if (later != Later || earlier != Earlier)
        {
            changed?.Invoke(this, new DataTableModelChangedEventArgs(DataTableChangeKind.Cursor));
        }

        if (summary != Summary)
        {
            changed?.Invoke(this, new DataTableModelChangedEventArgs(DataTableChangeKind.Summary));
        }
    }

    // How the rows held become the rows there are now: rows joined at the end or at the start, rows that stand but were drawn again, or all new.
    // The rows that are unchanged stay the same objects, so what was drawn for them is kept.
    private static (List<Models.TableRow> Rows, DataTableModelChangedEventArgs[] Events) Diff(List<Models.TableRow> before, List<Models.TableRow> after, IScreenFacts was, IScreenFacts now)
    {
        DataTableModelChangedEventArgs[] reset = [new DataTableModelChangedEventArgs(DataTableChangeKind.Reset)];
        if (before.Any(row => row.Id is null) || after.Any(row => row.Id is null) || before.Count == 0 || after.Count < before.Count)
        {
            return (after, reset);
        }

        int joined = after.Count - before.Count;
        int offset;
        if (Enumerable.Range(0, before.Count).All(at => before[at].Id == after[at].Id))
        {
            offset = 0;
        }
        else if (Enumerable.Range(0, before.Count).All(at => before[at].Id == after[joined + at].Id))
        {
            offset = joined;
        }
        else
        {
            return (after, reset);
        }

        List<DataTableModelChangedEventArgs> events = [];
        for (int at = 0; at < before.Count; at++)
        {
            if (PageRules.SameRow(before[at], after[offset + at]) && PageRules.SameAroundRow(after[offset + at], was, now))
            {
                after[offset + at] = before[at];
            }
            else
            {
                events.Add(new DataTableModelChangedEventArgs(DataTableChangeKind.Replace, offset + at, 1));
            }
        }

        if (joined > 0)
        {
            // The rows that joined are told first: the places of the rows drawn again are the places they have now.
            events.Insert(0, new DataTableModelChangedEventArgs(DataTableChangeKind.Insert, offset == 0 ? before.Count : 0, joined));
        }

        return (after, [.. events]);
    }

    /// <summary>Whether every record that matches is chosen.</summary>
    internal bool AcrossOn => state?.Across == true;

    /// <inheritdoc />
    public IReadOnlyList<DataTableColumn> Columns => columns;

    /// <inheritdoc />
    public int RowCount => rows.Count;

    /// <inheritdoc />
    public DataTableLoadState State => DataTableLoadState.Ready;

    /// <inheritdoc />
    public string? FailureMessage => null;

    /// <inheritdoc />
    public DataTableCursor Later => new(table.More is not null, state?.Failed == true ? CursorState.Failed : state?.Loading == true ? CursorState.Loading : CursorState.Idle);

    /// <inheritdoc />
    public DataTableCursor Earlier => new(table.Prev is not null, state?.FailedBefore == true ? CursorState.Failed : state?.LoadingBefore == true ? CursorState.Loading : CursorState.Idle);

    /// <inheritdoc />
    public string? Summary => table.Summary;

    /// <inheritdoc />
    public DataTableChoiceOptions Choice => new(table.Select is not null, table.Select?.Across is not null, table.Select?.Across?.Total);

    /// <inheritdoc />
    public DataTableRecord GetRow(int index)
    {
        Models.TableRow row = rows[index];
        Choosable choosable = table.Select is null || row.Id is null ? Choosable.None : row.Unchoosable ? Choosable.No : Choosable.Yes;
        bool edit = EditOf(row) is not null;
        bool menu = row.Actions.Any(node => node is Models.Button or Models.Menu) || edit;
        bool action = row.Action is not null || PrimaryOf(table, row) is not null;
        return new DataTableRecord(row.Id, choosable, action, menu, row.Detail.Count > 0, row.Open, edit, row);
    }

    /// <summary>What editing a row does: the first button of a form in its cells that has a field to fill, with the form's id.</summary>
    /// <param name="row">The row.</param>
    /// <returns>The button and the form's id, or null when the row has no form to send.</returns>
    internal static (Models.Button Button, string? Form)? EditOf(Models.TableRow row)
    {
        foreach (Models.Node cell in row.Cells)
        {
            if (ButtonIn(cell, null, mustFill: true) is { } found)
            {
                return found;
            }
        }

        return null;
    }

    /// <summary>What pressing a row does when it has no action of its own: the button its name cell holds, with the form it stands in.</summary>
    /// <param name="table">The table.</param>
    /// <param name="row">The row.</param>
    /// <returns>The button and the form's id, or null when the name cell holds no button.</returns>
    internal static (Models.Button Button, string? Form)? PrimaryOf(Models.Table table, Models.TableRow row)
    {
        int title = table.Columns.ToList().FindIndex(c => c.Role == Models.ColumnRole.Title);
        if (row.Cells.Count == 0 || row.Action is not null)
        {
            return null;
        }

        return ButtonIn(row.Cells[title < 0 ? 0 : Math.Min(title, row.Cells.Count - 1)], null, mustFill: false);
    }

    private static (Models.Button Button, string? Form)? ButtonIn(Models.Node node, string? form, bool mustFill)
    {
        switch (node)
        {
            case Models.Button { Action: not null } lone when !mustFill:
                return (lone, form);
            case Models.Form inner:
                string id = Tree.FormId(inner);
                if (mustFill && !HoldsField(inner))
                {
                    return null;
                }

                foreach (Models.Node child in inner.Children)
                {
                    if (ButtonIn(child, id, false) is { } found)
                    {
                        return found;
                    }
                }

                return null;
            case Models.Stack stack:
                foreach (Models.Node child in stack.Children)
                {
                    if (ButtonIn(child, form, mustFill) is { } inside)
                    {
                        return inside;
                    }
                }

                return null;
            default:
                return null;
        }
    }

    private static bool HoldsField(Models.Node node) => node switch
    {
        Models.Field => true,
        Models.Form form => form.Children.Any(HoldsField),
        Models.Stack stack => stack.Children.Any(HoldsField),
        _ => false,
    };

    /// <inheritdoc />
    public int IndexOfKey(string key) => places.TryGetValue(key, out int at) ? at : -1;

    private static DataTableColumn ColumnOf(Models.Column column, int index, IReadOnlyList<Models.TableRow> rows)
    {
        DesignRole role = column.Role switch
        {
            Models.ColumnRole.Leading => DesignRole.Leading,
            Models.ColumnRole.Title => DesignRole.Title,
            Models.ColumnRole.Subtitle => DesignRole.Subtitle,
            Models.ColumnRole.Status => DesignRole.Status,
            Models.ColumnRole.Trailing => DesignRole.Trailing,
            Models.ColumnRole.Actions => DesignRole.Actions,
            _ => DesignRole.None,
        };

        // A column the core gave no role is read by what its cells hold: all badges are a state, all controls are what is done to a row.
        if (role == DesignRole.None)
        {
            role = HoldsOnly(rows, index);
        }

        ColumnKind kind = role switch
        {
            DesignRole.Title => ColumnKind.Title,
            DesignRole.Leading => ColumnKind.Leading,
            DesignRole.Status => ColumnKind.Status,
            DesignRole.Actions => ColumnKind.Controls,
            _ => column.Numeric ? ColumnKind.Number : ColumnKind.Words,
        };
        DataTableAlign align = column.Align switch
        {
            Models.Align.Center => DataTableAlign.Center,
            Models.Align.End => DataTableAlign.End,
            Models.Align.Start => DataTableAlign.Start,
            _ => column.Numeric ? DataTableAlign.End : DataTableAlign.Start,
        };
        DataTableSort? sort = column.Sort is { } held
            ? new DataTableSort(held.Dir switch
            {
                Models.SortDir.Asc => DataTableSortDirection.Ascending,
                Models.SortDir.Desc => DataTableSortDirection.Descending,
                _ => DataTableSortDirection.None,
            }, held.Href)
            : null;
        return new DataTableColumn(column.Key ?? "#" + index.ToString(CultureInfo.InvariantCulture) + ":" + column.Label, column.Label, kind, role, align, sort, column.Secondary);
    }

    private static DesignRole HoldsOnly(IReadOnlyList<Models.TableRow> rows, int index)
    {
        int seen = 0;
        bool badges = true;
        bool controls = true;
        for (int r = 0; r < rows.Count && r < Sample; r++)
        {
            if (index >= rows[r].Cells.Count)
            {
                continue;
            }

            Models.Node cell = rows[r].Cells[index];
            seen++;
            badges &= cell is Models.Badge;
            controls &= OnlyControls(cell);
        }

        return seen == 0 ? DesignRole.None : badges ? DesignRole.Status : controls ? DesignRole.Actions : DesignRole.None;
    }

    private static bool OnlyControls(Models.Node node) => node switch
    {
        Models.Button or Models.Menu or Models.Field => true,
        Models.Form form => form.Children.Count > 0 && form.Children.All(OnlyControls),
        Models.Stack stack => stack.Children.Count > 0 && stack.Children.All(OnlyControls),
        _ => false,
    };
}

internal static class TextBlockWidth
{
    /// <summary>The width of the words of a text block in one line.</summary>
    /// <param name="block">The text block.</param>
    /// <returns>The width in effective pixels.</returns>
    internal static double MeasuredWidth(this TextBlock block)
    {
        block.Measure(new Windows.Foundation.Size(double.PositiveInfinity, double.PositiveInfinity));
        return Math.Ceiling(block.DesiredSize.Width);
    }
}

/// <summary>Draws a protocol table's cells: each is a node, drawn by the shell as it draws it anywhere.</summary>
internal sealed class ProtocolTablePresenter(
    Models.Table initial,
    Func<Models.Node, UIElement> render,
    Func<IEnumerable<Models.Node>, MenuFlyout> menu,
    Func<IEnumerable<Models.Node>, UIElement> column,
    Action<Models.Button, string?> submit) : IDataTablePresenter
{
    /// <summary>The table the cells are drawn for, as it stands now: its columns and the first rows say what is looked for in the rest.</summary>
    internal Models.Table Table { get; set; } = initial;

    // A mark and the gap after it, beside the words of a state.
    private const double MarkWidth = 16;

    private readonly Dictionary<(Models.TableRow Row, int Column), UIElement> drawn = [];
    private readonly Queue<(Models.TableRow Row, int Column)> drawnOrder = [];
    private const int DrawnLeast = 600;

    /// <inheritdoc />
    public UIElement CreateCell(DataTableColumn definition, RowShape shape) =>
        new Border { VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = definition.Align == DataTableAlign.End ? HorizontalAlignment.Right : HorizontalAlignment.Left };

    /// <inheritdoc />
    public void BindCell(UIElement cell, in DataTableRecord record, int columnIndex)
    {
        if (cell is not Border holder || record.Data is not Models.TableRow row || columnIndex >= row.Cells.Count)
        {
            return;
        }

        UIElement element = Element(row, columnIndex);
        if (element is FrameworkElement { Parent: Border previous } && !ReferenceEquals(previous, holder))
        {
            previous.Child = null;
        }

        holder.Child = element;
    }

    /// <inheritdoc />
    public void ClearCell(UIElement cell)
    {
        if (cell is Border holder)
        {
            holder.Child = null;
        }
    }

    /// <inheritdoc />
    public string CellText(in DataTableRecord record, int columnIndex) =>
        record.Data is Models.TableRow row && columnIndex < row.Cells.Count ? string.Join(", ", Lines(row.Cells[columnIndex])) : string.Empty;

    /// <inheritdoc />
    public string? SecondLine(in DataTableRecord record)
    {
        if (record.Data is not Models.TableRow row)
        {
            return null;
        }

        int title = Table.Columns.ToList().FindIndex(c => c.Role == Models.ColumnRole.Title);
        List<string> lines = title >= 0 && title < row.Cells.Count ? Lines(row.Cells[title]) : [];
        return lines.Count > 1 ? lines[1] : null;
    }

    /// <inheritdoc />
    public string RowName(in DataTableRecord record)
    {
        if (record.Data is not Models.TableRow row)
        {
            return record.Key ?? string.Empty;
        }

        int title = Table.Columns.ToList().FindIndex(c => c.Role == Models.ColumnRole.Title);
        List<string> lines = row.Cells.Count == 0 ? [] : Lines(row.Cells[Math.Max(title, 0)]);
        return lines.Count > 0 ? lines[0] : record.Key ?? string.Empty;
    }

    /// <inheritdoc />
    public int ControlCount(in DataTableRecord record, int columnIndex) =>
        record.Data is Models.TableRow row && columnIndex < row.Cells.Count ? Controls(row.Cells[columnIndex]) : 0;

    /// <inheritdoc />
    public double ControlsWidth(in DataTableRecord record, int columnIndex)
    {
        if (record.Data is not Models.TableRow row || columnIndex >= row.Cells.Count)
        {
            return 0;
        }

        // What the controls ask for when nothing limits them: the column is as wide as the widest row's. A control that is
        // not on the screen yet may measure short of what it will be, so what its words and marks need is the least it gets.
        UIElement element = Element(row, columnIndex);
        Prepare(element);
        element.Measure(new Windows.Foundation.Size(double.PositiveInfinity, double.PositiveInfinity));
        return Math.Max(element.DesiredSize.Width, Needed(row.Cells[columnIndex]));
    }

    private static void Prepare(DependencyObject element)
    {
        if (element is Control control)
        {
            control.ApplyTemplate();
        }

        switch (element)
        {
            case Panel panel:
                foreach (UIElement child in panel.Children)
                {
                    Prepare(child);
                }

                break;
            case ContentControl { Content: DependencyObject inner }:
                Prepare(inner);
                break;
            default:
                break;
        }
    }

    // The width the controls of a node need side by side: a button its words, its mark and its padding, a number field its width.
    private static double Needed(Models.Node node)
    {
        List<double> widths = [];
        Gather(node, widths);
        return widths.Count == 0 ? 0 : widths.Sum() + ((widths.Count - 1) * 8);
    }

    private static void Gather(Models.Node node, List<double> widths)
    {
        switch (node)
        {
            case Models.Button button:
                double words = button.IconOnly ? 0 : new TextBlock { Text = button.Label }.MeasuredWidth();
                bool mark = button.Icon is not null || button.Standard is not null || button.Pending;
                widths.Add(words + (mark ? (button.IconOnly ? 16 : 24) : 0) + 24);
                break;
            case Models.Menu menu:
                widths.Add(new TextBlock { Text = menu.Label }.MeasuredWidth() + (menu.Icon is not null ? 24 : 0) + 24 + 20);
                break;
            case Models.Field:
                widths.Add(88);
                break;
            case Models.Form form:
                foreach (Models.Node child in form.Children)
                {
                    Gather(child, widths);
                }

                break;
            case Models.Stack stack:
                foreach (Models.Node child in stack.Children)
                {
                    Gather(child, widths);
                }

                break;
            default:
                break;
        }
    }

    /// <inheritdoc />
    public double BesideWords(int columnIndex) =>
        columnIndex < Table.Columns.Count && Table.Rows.Take(40).Any(row => columnIndex < row.Cells.Count && row.Cells[columnIndex] is Models.Badge { Dot: true }) ? MarkWidth : 0;

    /// <inheritdoc />
    public UIElement? CreateDetail(in DataTableRecord record) =>
        record.Data is Models.TableRow { Detail.Count: > 0 } row ? column(row.Detail) : null;

    /// <inheritdoc />
    public FlyoutBase? CreateRowMenu(in DataTableRecord record)
    {
        if (record.Data is not Models.TableRow row)
        {
            return null;
        }

        MenuFlyout flyout = row.Actions.Count > 0 ? menu(row.Actions) : new MenuFlyout();

        // What a row's own form does (restock, with the number it holds) is in its menu too, for a narrow table that shows no form.
        if (ProtocolTableModel.EditOf(row) is { } edit)
        {
            MenuFlyoutItem item = new() { Text = edit.Button.Label };
            item.Click += (_, _) => submit(edit.Button, edit.Form);
            flyout.Items.Add(item);
        }

        return flyout.Items.Count > 0 ? flyout : null;
    }

    // A cell drawn once and kept: the row binds it again when it comes back.
    private UIElement Element(Models.TableRow row, int index)
    {
        if (!drawn.TryGetValue((row, index), out UIElement? element))
        {
            Models.Node node = row.Cells[index];
            DataTableAlign align = index < Table.Columns.Count ? AlignOf(Table.Columns[index]) : DataTableAlign.Start;
            element = render(node);
            Settle(element, align);
            drawn[(row, index)] = element;
            drawnOrder.Enqueue((row, index));

            // The cells of the rows that went by long ago are drawn again if those rows come back: the table keeps the latest few hundred, not all.
            while (drawn.Count > DrawnLeast && drawnOrder.TryDequeue(out (Models.TableRow Row, int Column) oldest))
            {
                _ = drawn.Remove(oldest);
            }
        }

        return element;
    }

    private static DataTableAlign AlignOf(Models.Column definition) => definition.Align switch
    {
        Models.Align.Center => DataTableAlign.Center,
        Models.Align.End => DataTableAlign.End,
        Models.Align.Start => DataTableAlign.Start,
        _ => definition.Numeric ? DataTableAlign.End : DataTableAlign.Start,
    };

    // What the shell draws for a page is made fit for a cell: fields lose their label (it is their name), a text button
    // stands flush with the words above and under it, and what stacks in an end column ends at its edge.
    private static void Settle(DependencyObject element, DataTableAlign align)
    {
        switch (element)
        {
            case TextBox box:
                if (box.Header is string header)
                {
                    AutomationProperties.SetName(box, header);
                }

                box.Header = null;
                box.Width = 88;
                break;
            case TextBlock words:
                // Words that do not fit are cut with an ellipsis and are all in a tooltip, never cut without a sign.
                words.TextWrapping = TextWrapping.NoWrap;
                words.TextTrimming = TextTrimming.CharacterEllipsis;
                words.IsTextTrimmedChanged += (sender, _) => ToolTipService.SetToolTip(sender, sender.IsTextTrimmed ? sender.Text : null);
                break;
            case ComboBox combo:
                if (combo.Header is string named)
                {
                    AutomationProperties.SetName(combo, named);
                }

                combo.Header = null;
                break;
            case Button button when ReferenceEquals(button.Style, Look.Ramp("TextBlockButtonStyle")):
                // The words stand where they would with no padding; the padding is taken back by the margin, so the button's own area is a
                // line and a half tall (a pointer's target at least 24 pixels) and nothing moves.
                button.Padding = new Thickness(0, 3, 0, 3);
                button.Margin = new Thickness(0, -3, 0, -3);
                button.MinWidth = 0;
                button.MinHeight = 24;
                button.HorizontalAlignment = HorizontalAlignment.Left;
                button.HorizontalContentAlignment = HorizontalAlignment.Left;
                break;
            case Panel panel:
                // A row of one part that is as wide as its words would never cut them: it takes the room it has, and the words end in an ellipsis.
                if (panel is Grid { ColumnDefinitions.Count: 1 } single && single.ColumnDefinitions[0].Width.IsAuto)
                {
                    single.ColumnDefinitions[0].Width = new GridLength(1, GridUnitType.Star);
                }
                else if (panel is Grid { ColumnDefinitions.Count: > 1 } several
                    && several.ColumnDefinitions[^1].Width.IsAuto
                    && several.Children.OfType<TextBlock>().Any(words => Grid.GetColumn(words) == several.ColumnDefinitions.Count - 1))
                {
                    // A flag and a place, a mark and a word: the words take what is left of the cell and end in an ellipsis.
                    several.ColumnDefinitions[^1].Width = new GridLength(1, GridUnitType.Star);
                }

                foreach (UIElement child in panel.Children)
                {
                    Settle(child, align);
                }

                if (panel is StackPanel { Orientation: Orientation.Vertical } stack)
                {
                    HorizontalAlignment edge = align switch
                    {
                        DataTableAlign.End => HorizontalAlignment.Right,
                        DataTableAlign.Center => HorizontalAlignment.Center,
                        _ => HorizontalAlignment.Left,
                    };
                    foreach (UIElement child in stack.Children)
                    {
                        if (child is FrameworkElement placed && child is not TextBox)
                        {
                            placed.HorizontalAlignment = edge;
                        }
                    }
                }

                break;
            default:
                break;
        }
    }

    // The words a node says, one for each line: text, a badge, a button's label, what a stack or a form holds.
    internal static List<string> Lines(Models.Node node)
    {
        switch (node)
        {
            case Models.Text text:
                return [text.Value];
            case Models.Badge badge:
                return [badge.Text];
            case Models.Button button:
                return [button.Label];
            case Models.Link link:
                return [link.Label];
            case Models.Form form:
                return form.Children.SelectMany(Lines).ToList();
            case Models.Stack stack:
                {
                    List<List<string>> parts = stack.Children.Select(Lines).Where(l => l.Count > 0).ToList();
                    bool vertical = stack.Axis != Models.Axis.Horizontal;
                    if (vertical || parts.Count(p => p.Count > 1) == 1)
                    {
                        return parts.SelectMany(p => p).ToList();
                    }

                    return parts.Count == 0 ? [] : [string.Join(" ", parts.Select(p => p[0]))];
                }

            default:
                return [];
        }
    }

    private static int Controls(Models.Node node) => node switch
    {
        Models.Button or Models.Menu or Models.Field => 1,
        Models.Form form => form.Children.Sum(Controls),
        Models.Stack stack => stack.Children.Sum(Controls),
        _ => 0,
    };
}
