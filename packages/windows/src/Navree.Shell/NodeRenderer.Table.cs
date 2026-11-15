using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Navree.Controls;
using Navree.Design;
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

    private UIElement DrawTable(Models.Table table)
    {
        ProtocolTableModel model = new(table, context.Screen);
        ProtocolTablePresenter presenter = new(table, Render, MenuFor, nodes => Column(nodes));
        DataTable view = new()
        {
            Model = model,
            Presenter = presenter,
            Caption = table.Caption,
            Culture = CultureInfo.CurrentCulture,
            Height = TableHeight,
            VerticalAlignment = VerticalAlignment.Top,
        };

        if (table.Empty is not null)
        {
            view.EmptyContent = Render(table.Empty);
        }

        string? id = table.Id;
        long screen = context.Screen.Id;
        Dictionary<string, Models.TableRow> byKey = table.Rows.Where(row => row.Id is not null).ToDictionary(row => row.Id!, StringComparer.Ordinal);
        view.ApplyChoice(byKey.Values.Where(row => row.Selected).Select(row => row.Id!), model.AcrossOn);
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
                Act(new Models.ActionNavigate { Href = sort.Address, Present = Models.Present.Replace }, null, null);
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
            if (byKey.TryGetValue(e.Key, out Models.TableRow? row))
            {
                Act(row.Action, null, row.Id);
            }
        };
        view.DetailToggled += (_, e) => context.Send(Events.Overlay(screen, e.Key, e.Open));

        StackPanel panel = new() { Spacing = 8 };
        if (table.Caption is not null)
        {
            panel.Children.Add(Look.Label(table.Caption, "BodyStrongTextBlockStyle"));
        }

        if (table.Query is not null && TableToolbar(table.Query) is { } toolbar)
        {
            view.Toolbar = toolbar;
        }

        panel.Children.Add(view);
        return panel;
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
            Button filter = new()
            {
                Content = "Filter",
                VerticalAlignment = VerticalAlignment.Bottom,
                Flyout = new Flyout { Content = new ScrollViewer { Content = fields, MaxHeight = 480 }, Placement = FlyoutPlacementMode.BottomEdgeAlignedRight },
            };
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

/// <summary>A protocol table as the table control reads it: a snapshot of the rows the core sent, with the state of its cursor.</summary>
internal sealed class ProtocolTableModel : IDataTableModel
{
    private readonly Models.Table table;
    private readonly DataTableColumn[] columns;
    private readonly Dictionary<string, int> places = new(StringComparer.Ordinal);
    private readonly ListState? state;

    internal ProtocolTableModel(Models.Table protocol, ScreenView screen)
    {
        table = protocol;
        state = protocol.Id is { } id && screen.Lists.TryGetValue(id, out ListState? held) ? held : null;
        columns = protocol.Columns.Select(ColumnOf).ToArray();
        for (int i = 0; i < protocol.Rows.Count; i++)
        {
            if (protocol.Rows[i].Id is { } key)
            {
                places[key] = i;
            }
        }
    }

    /// <inheritdoc />
    public event EventHandler<DataTableModelChangedEventArgs>? Changed
    {
        add { }
        remove { }
    }

    /// <summary>Whether every record that matches is chosen.</summary>
    internal bool AcrossOn => state?.Across == true;

    /// <inheritdoc />
    public IReadOnlyList<DataTableColumn> Columns => columns;

    /// <inheritdoc />
    public int RowCount => table.Rows.Count;

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
        Models.TableRow row = table.Rows[index];
        Choosable choosable = table.Select is null || row.Id is null ? Choosable.None : row.Unchoosable ? Choosable.No : Choosable.Yes;
        bool menu = row.Actions.Any(node => node is Models.Button or Models.Menu);
        return new DataTableRecord(row.Id, choosable, row.Action is not null, menu, row.Detail.Count > 0, row.Open, false, row);
    }

    /// <inheritdoc />
    public int IndexOfKey(string key) => places.TryGetValue(key, out int at) ? at : -1;

    private static DataTableColumn ColumnOf(Models.Column column, int index)
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
}

/// <summary>Draws a protocol table's cells: each is a node, drawn by the shell as it draws it anywhere.</summary>
internal sealed class ProtocolTablePresenter(
    Models.Table table,
    Func<Models.Node, UIElement> render,
    Func<IEnumerable<Models.Node>, MenuFlyout> menu,
    Func<IEnumerable<Models.Node>, UIElement> column) : IDataTablePresenter
{
    private readonly Dictionary<(Models.TableRow Row, int Column), UIElement> drawn = [];

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

        if (!drawn.TryGetValue((row, columnIndex), out UIElement? element))
        {
            element = render(row.Cells[columnIndex]);
            drawn[(row, columnIndex)] = element;
        }

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
        record.Data is Models.TableRow row && columnIndex < row.Cells.Count ? string.Join(" ", Lines(row.Cells[columnIndex])) : string.Empty;

    /// <inheritdoc />
    public string? SecondLine(in DataTableRecord record)
    {
        if (record.Data is not Models.TableRow row)
        {
            return null;
        }

        int title = table.Columns.ToList().FindIndex(c => c.Role == Models.ColumnRole.Title);
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

        int title = table.Columns.ToList().FindIndex(c => c.Role == Models.ColumnRole.Title);
        List<string> lines = row.Cells.Count == 0 ? [] : Lines(row.Cells[Math.Max(title, 0)]);
        return lines.Count > 0 ? lines[0] : record.Key ?? string.Empty;
    }

    /// <inheritdoc />
    public int ControlCount(in DataTableRecord record, int columnIndex) =>
        record.Data is Models.TableRow row && columnIndex < row.Cells.Count ? Controls(row.Cells[columnIndex]) : 0;

    /// <inheritdoc />
    public UIElement? CreateDetail(in DataTableRecord record) =>
        record.Data is Models.TableRow { Detail.Count: > 0 } row ? column(row.Detail) : null;

    /// <inheritdoc />
    public FlyoutBase? CreateRowMenu(in DataTableRecord record) =>
        record.Data is Models.TableRow row && row.Actions.Count > 0 ? menu(row.Actions) : null;

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
