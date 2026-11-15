using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Automation.Provider;
using Navree.Design.Tables;

namespace Navree.Controls;

public sealed partial class DataTable
{
    /// <inheritdoc />
    protected override AutomationPeer OnCreateAutomationPeer() => new DataTableAutomationPeer(this);

    // Says something to assistive technology without moving focus.
    private void RaiseNotification(string words)
    {
        if (string.IsNullOrEmpty(words))
        {
            return;
        }

        if (FrameworkElementAutomationPeer.FromElement(this) is { } peer)
        {
            peer.RaiseNotificationEvent(
                AutomationNotificationKind.ActionCompleted,
                AutomationNotificationProcessing.ImportantMostRecent,
                words,
                "DataTable");
        }
    }

    /// <summary>The rows that are made, in order of their place.</summary>
    internal IEnumerable<DataTableRow> RealizedRows() => liveRows.Where(r => r.Index >= 0).OrderBy(r => r.Index);

    /// <summary>Makes one row the only one chosen, as assistive technology asks.</summary>
    /// <param name="index">The row's place.</param>
    internal void ChooseOnly(int index)
    {
        if (ChosenCount > 0 || choice.Across)
        {
            ClearChoice();
        }

        ToggleChoice(index);
    }

    /// <summary>Runs a row's action, as assistive technology asks.</summary>
    /// <param name="row">The row.</param>
    internal void InvokeRow(DataTableRow row)
    {
        if (row.Key is { } key && row.Record.HasAction)
        {
            RaiseRowInvoked(key);
        }
    }

    /// <summary>Scrolls a row into view, as assistive technology asks.</summary>
    /// <param name="index">The row's place.</param>
    internal void ScrollToRowIndex(int index) => RevealRow(index);

    /// <summary>The words a sort puts beside the table's name.</summary>
    internal string? SortStatus()
    {
        if (Model is not { } model)
        {
            return null;
        }

        foreach (DataTableColumn column in model.Columns)
        {
            if (column.Sort is { Direction: not DataTableSortDirection.None } sort)
            {
                return DataTableStrings.Get(sort.Direction == DataTableSortDirection.Ascending ? "DataTable_Sorted_Ascending" : "DataTable_Sorted_Descending", column.Header);
            }
        }

        return null;
    }
}

/// <summary>What assistive technology is told of the table: a data grid with a header, rows and cells.</summary>
public sealed partial class DataTableAutomationPeer : FrameworkElementAutomationPeer, IGridProvider, ITableProvider, ISelectionProvider, IItemContainerProvider
{
    private readonly DataTable table;
    private readonly System.Runtime.CompilerServices.ConditionalWeakTable<IRawElementProviderSimple, string> providerKeys = new();

    /// <summary>Makes the peer.</summary>
    /// <param name="owner">The table.</param>
    public DataTableAutomationPeer(DataTable owner)
        : base(owner)
    {
        table = owner;
    }

    /// <inheritdoc />
    public int RowCount => table.Model?.RowCount ?? 0;

    /// <inheritdoc />
    public int ColumnCount => table.Shown.Count;

    /// <inheritdoc />
    public RowOrColumnMajor RowOrColumnMajor => RowOrColumnMajor.RowMajor;

    /// <inheritdoc />
    public bool CanSelectMultiple => true;

    /// <inheritdoc />
    public bool IsSelectionRequired => false;

    /// <inheritdoc />
    public IRawElementProviderSimple GetItem(int row, int column)
    {
        if (table.RealizedRows().FirstOrDefault(r => r.Index == row) is { } made && column >= 0 && column < made.Cells.Count)
        {
            return ProviderFromPeer(FrameworkElementAutomationPeer.CreatePeerForElement(made.Cells[column]));
        }

        return RowProvider(row);
    }

    // A row's provider: its own peer when it is made, else a stand-in that can ask for it to be made.
    private IRawElementProviderSimple RowProvider(int index)
    {
        if (table.Model is not { } model || index < 0 || index >= model.RowCount || model.GetRow(index).Key is not { } key)
        {
            return null!;
        }

        DataTableRow? made = table.RealizedRows().FirstOrDefault(r => r.Index == index);
        IRawElementProviderSimple provider = made is not null
            ? ProviderFromPeer(FrameworkElementAutomationPeer.CreatePeerForElement(made))
            : ProviderFromPeer(new DataTableRowStandInAutomationPeer(table, index, key));
        providerKeys.AddOrUpdate(provider, key);
        return provider;
    }

    /// <summary>The stand-in for a row that is not made, which can ask for it to be made.</summary>
    /// <param name="index">The row's place.</param>
    /// <returns>The stand-in, or null when there is no such row.</returns>
    internal DataTableRowStandInAutomationPeer? RowStandIn(int index) =>
        table.Model is { } model && index >= 0 && index < model.RowCount && model.GetRow(index).Key is { } key
            ? new DataTableRowStandInAutomationPeer(table, index, key)
            : null;

    /// <inheritdoc />
    public IRawElementProviderSimple[] GetColumnHeaders() =>
        table.HeaderRow is { } header
            ? header.Headers.Select(h => ProviderFromPeer(FrameworkElementAutomationPeer.CreatePeerForElement(h))).ToArray()
            : [];

    /// <inheritdoc />
    public IRawElementProviderSimple[] GetRowHeaders() => [];

    /// <inheritdoc />
    public IRawElementProviderSimple[] GetSelection() =>
        table.RealizedRows()
            .Where(r => r.Key is not null && table.IsChosen(r.Key, r.Record.Choosable))
            .Select(r => ProviderFromPeer(FrameworkElementAutomationPeer.CreatePeerForElement(r)))
            .ToArray();

    /// <inheritdoc />
    public IRawElementProviderSimple FindItemByProperty(IRawElementProviderSimple startAfter, AutomationProperty automationProperty, object value)
    {
        if (table.Model is not { } model)
        {
            return null!;
        }

        // With no property, the next row after the one given, or the first.
        if (automationProperty is null)
        {
            int after = -1;
            if (startAfter is not null && providerKeys.TryGetValue(startAfter, out string? startKey))
            {
                after = model.IndexOfKey(startKey);
            }

            return RowProvider(after + 1);
        }

        if (automationProperty == AutomationElementIdentifiers.AutomationIdProperty && value is string key)
        {
            return RowProvider(model.IndexOfKey(key));
        }

        if (automationProperty == AutomationElementIdentifiers.NameProperty && value is string name && table.Presenter is { } presenter)
        {
            for (int i = 0; i < model.RowCount; i++)
            {
                if (string.Equals(presenter.RowName(model.GetRow(i)), name, StringComparison.Ordinal))
                {
                    return RowProvider(i);
                }
            }
        }

        return null!;
    }

    /// <summary>The peer's provider, for the parts that name their container.</summary>
    internal IRawElementProviderSimple Provider => ProviderFromPeer(this);

    /// <inheritdoc />
    protected override object? GetPatternCore(PatternInterface patternInterface)
    {
        switch (patternInterface)
        {
            case PatternInterface.Grid:
            case PatternInterface.Table:
                return table.IsList ? null : this;
            case PatternInterface.ItemContainer:
                return this;
            case PatternInterface.Selection when table.Model?.Choice.Available == true:
                return this;
            case PatternInterface.Scroll when table.Scroller is { } scroller:
                return FrameworkElementAutomationPeer.CreatePeerForElement(scroller).GetPattern(PatternInterface.Scroll);
            default:
                return base.GetPatternCore(patternInterface);
        }
    }

    /// <inheritdoc />
    protected override IList<AutomationPeer> GetChildrenCore()
    {
        List<AutomationPeer> children = [];
        if (table.HeaderRow is { } header)
        {
            children.Add(FrameworkElementAutomationPeer.CreatePeerForElement(header));
        }

        foreach (DataTableRow row in table.RealizedRows())
        {
            children.Add(FrameworkElementAutomationPeer.CreatePeerForElement(row));
        }

        return children;
    }

    /// <inheritdoc />
    protected override AutomationControlType GetAutomationControlTypeCore() => table.IsList ? AutomationControlType.List : AutomationControlType.DataGrid;

    /// <inheritdoc />
    protected override string GetClassNameCore() => nameof(DataTable);

    /// <inheritdoc />
    protected override string GetNameCore() =>
        string.IsNullOrEmpty(table.Caption) ? DataTableStrings.Get("DataTable_Name_Default") : table.Caption;

    /// <inheritdoc />
    protected override string GetHelpTextCore() => table.Model?.Summary ?? string.Empty;

    /// <inheritdoc />
    protected override string GetItemStatusCore() => table.SortStatus() ?? string.Empty;

    /// <inheritdoc />
    protected override string GetLocalizedControlTypeCore() => DataTableStrings.Get("DataTable_ControlType_Table");
}

/// <summary>The row of headings, told to assistive technology as a header.</summary>
public sealed class DataTableHeaderAutomationPeer : FrameworkElementAutomationPeer
{
    private readonly DataTableRow row;

    /// <summary>Makes the peer.</summary>
    /// <param name="owner">The row of headings.</param>
    public DataTableHeaderAutomationPeer(DataTableRow owner)
        : base(owner)
    {
        row = owner;
    }

    /// <inheritdoc />
    protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.Header;

    /// <inheritdoc />
    protected override string GetClassNameCore() => "DataTableHeader";

    /// <inheritdoc />
    protected override string GetNameCore() => DataTableStrings.Get("DataTable_Header_Row");

    /// <inheritdoc />
    protected override IList<AutomationPeer> GetChildrenCore() =>
        row.Headers.Select(h => (AutomationPeer)FrameworkElementAutomationPeer.CreatePeerForElement(h)).ToList();
}

/// <summary>A heading, told to assistive technology as a header item that can sort and be resized.</summary>
public sealed partial class DataTableHeaderItemAutomationPeer : FrameworkElementAutomationPeer, IInvokeProvider, ITransformProvider
{
    private readonly DataTableHeaderCell cell;

    /// <summary>Makes the peer.</summary>
    /// <param name="owner">The heading.</param>
    public DataTableHeaderItemAutomationPeer(DataTableHeaderCell owner)
        : base(owner)
    {
        cell = owner;
    }

    /// <inheritdoc />
    public bool CanMove => false;

    /// <inheritdoc />
    public bool CanResize => cell.Grip?.Visibility == Visibility.Visible;

    /// <inheritdoc />
    public bool CanRotate => false;

    /// <inheritdoc />
    public void Invoke() => cell.Row?.Owner?.SortPressed(cell);

    /// <inheritdoc />
    public void Move(double x, double y)
    {
    }

    /// <inheritdoc />
    public void Resize(double width, double height) => cell.Row?.Owner?.ResizeTo(cell, width);

    /// <inheritdoc />
    public void Rotate(double degrees)
    {
    }

    /// <inheritdoc />
    protected override object? GetPatternCore(PatternInterface patternInterface) => patternInterface switch
    {
        PatternInterface.Invoke when cell.Sortable => this,
        PatternInterface.Transform when CanResize => this,
        _ => base.GetPatternCore(patternInterface),
    };

    /// <inheritdoc />
    protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.HeaderItem;

    /// <inheritdoc />
    protected override string GetClassNameCore() => nameof(DataTableHeaderCell);

    /// <inheritdoc />
    protected override string GetNameCore() => string.IsNullOrEmpty(cell.Heading) ? "Actions" : cell.Heading;

    /// <inheritdoc />
    protected override string GetAutomationIdCore() => "col:" + cell.ColumnKey;

    /// <inheritdoc />
    protected override string GetItemStatusCore() => cell.SortWords ?? string.Empty;

    /// <inheritdoc />
    protected override string GetHelpTextCore() => cell.Sortable ? DataTableStrings.Get("DataTable_Sortable_Help") : string.Empty;

    /// <inheritdoc />
    protected override string GetLocalizedControlTypeCore() => DataTableStrings.Get("DataTable_ControlType_ColumnHeader");
}

/// <summary>A row, told to assistive technology as an item that can be chosen, invoked and opened.</summary>
public sealed partial class DataTableRowAutomationPeer : FrameworkElementAutomationPeer, ISelectionItemProvider, IInvokeProvider, IExpandCollapseProvider, IScrollItemProvider
{
    private readonly DataTableRow row;

    /// <summary>Makes the peer.</summary>
    /// <param name="owner">The row.</param>
    public DataTableRowAutomationPeer(DataTableRow owner)
        : base(owner)
    {
        row = owner;
    }

    private DataTable? Table => row.Owner;

    /// <inheritdoc />
    public bool IsSelected => row.Key is { } key && Table?.IsChosen(key, row.Record.Choosable) == true;

    /// <inheritdoc />
    public IRawElementProviderSimple SelectionContainer =>
        Table is { } table ? ProviderFromPeer(FrameworkElementAutomationPeer.CreatePeerForElement(table)) : null!;

    /// <inheritdoc />
    public ExpandCollapseState ExpandCollapseState =>
        !row.Record.HasDetail ? ExpandCollapseState.LeafNode : Table?.IsDetailOpen(row.Key) == true ? ExpandCollapseState.Expanded : ExpandCollapseState.Collapsed;

    /// <inheritdoc />
    public void AddToSelection()
    {
        if (!IsSelected)
        {
            Table?.ToggleChoice(row.Index);
        }
    }

    /// <inheritdoc />
    public void RemoveFromSelection()
    {
        if (IsSelected)
        {
            Table?.ToggleChoice(row.Index);
        }
    }

    /// <inheritdoc />
    public void Select() => Table?.ChooseOnly(row.Index);

    /// <inheritdoc />
    public void Invoke() => Table?.InvokeRow(row);

    /// <inheritdoc />
    public void Expand()
    {
        if (row.Record.HasDetail && Table?.IsDetailOpen(row.Key) != true)
        {
            Table?.ToggleDetail(row);
        }
    }

    /// <inheritdoc />
    public void Collapse()
    {
        if (row.Record.HasDetail && Table?.IsDetailOpen(row.Key) == true)
        {
            Table?.ToggleDetail(row);
        }
    }

    /// <inheritdoc />
    public void ScrollIntoView() => Table?.ScrollToRowIndex(row.Index);

    /// <inheritdoc />
    protected override object? GetPatternCore(PatternInterface patternInterface)
    {
        switch (patternInterface)
        {
            case PatternInterface.SelectionItem when Table?.Model?.Choice.Available == true:
            case PatternInterface.ScrollItem:
                return this;
            case PatternInterface.Invoke when row.Record.HasAction:
                return this;
            case PatternInterface.ExpandCollapse when row.Record.HasDetail:
                return this;
            default:
                return base.GetPatternCore(patternInterface);
        }
    }

    /// <inheritdoc />
    protected override AutomationControlType GetAutomationControlTypeCore() =>
        Table?.IsList == true ? AutomationControlType.ListItem : AutomationControlType.DataItem;

    /// <inheritdoc />
    protected override string GetClassNameCore() => nameof(DataTableRow);

    /// <inheritdoc />
    protected override string GetNameCore()
    {
        DataTable? table = Table;
        if (table?.Presenter is not { } presenter)
        {
            return row.Key ?? string.Empty;
        }

        if (table.IsList)
        {
            return table.ListWords(row.Record);
        }

        List<string> words = [presenter.RowName(row.Record)];
        foreach (DataTableCell cell in row.Cells.Where(c => c.ColumnIndex >= 0))
        {
            string text = presenter.CellText(row.Record, cell.ColumnIndex);
            if (text.Length > 0 && !words.Contains(text))
            {
                words.Add(text);
            }
        }

        return string.Join(", ", words);
    }

    /// <inheritdoc />
    protected override string GetAutomationIdCore() => row.Key ?? string.Empty;

    /// <inheritdoc />
    protected override int GetPositionInSetCore() => row.Index + 1;

    /// <inheritdoc />
    protected override int GetSizeOfSetCore() => Table?.Model?.RowCount ?? 0;

    /// <inheritdoc />
    protected override string GetLocalizedControlTypeCore() => DataTableStrings.Get("DataTable_ControlType_Row");

    /// <inheritdoc />
    protected override IList<AutomationPeer> GetChildrenCore() =>
        row.Cells.Select(c => (AutomationPeer)FrameworkElementAutomationPeer.CreatePeerForElement(c)).ToList();
}

/// <summary>A cell, told to assistive technology as a grid item with its row, column and heading.</summary>
public sealed partial class DataTableCellAutomationPeer : FrameworkElementAutomationPeer, IGridItemProvider, ITableItemProvider
{
    private readonly DataTableCell cell;

    /// <summary>Makes the peer.</summary>
    /// <param name="owner">The cell.</param>
    public DataTableCellAutomationPeer(DataTableCell owner)
        : base(owner)
    {
        cell = owner;
    }

    private DataTable? Table => cell.Row?.Owner;

    /// <inheritdoc />
    public int Column => Math.Max(cell.GridColumn, 0);

    /// <inheritdoc />
    public int ColumnSpan => 1;

    /// <inheritdoc />
    public int Row => cell.Row?.Index ?? -1;

    /// <inheritdoc />
    public int RowSpan => 1;

    /// <inheritdoc />
    public IRawElementProviderSimple ContainingGrid =>
        Table is { } table ? ProviderFromPeer(FrameworkElementAutomationPeer.CreatePeerForElement(table)) : null!;

    /// <inheritdoc />
    public IRawElementProviderSimple[] GetColumnHeaderItems() =>
        Table?.HeaderRow?.Headers.FirstOrDefault(h => h.GridColumn == cell.GridColumn) is { } header
            ? [ProviderFromPeer(FrameworkElementAutomationPeer.CreatePeerForElement(header))]
            : [];

    /// <inheritdoc />
    public IRawElementProviderSimple[] GetRowHeaderItems() => [];

    /// <inheritdoc />
    protected override object? GetPatternCore(PatternInterface patternInterface) =>
        patternInterface is PatternInterface.GridItem or PatternInterface.TableItem && cell.Role == CellRole.Data
            ? this
            : base.GetPatternCore(patternInterface);

    /// <inheritdoc />
    protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.DataItem;

    /// <inheritdoc />
    protected override string GetClassNameCore() => nameof(DataTableCell);

    /// <inheritdoc />
    protected override string GetNameCore() =>
        Table?.Presenter is { } presenter && cell.Row is { } row && cell.Role == CellRole.Data
            ? (cell.ColumnIndex >= 0 ? presenter.CellText(row.Record, cell.ColumnIndex) : Table.ListWords(row.Record))
            : string.Empty;

    /// <inheritdoc />
    protected override string GetAutomationIdCore() =>
        cell.Row?.Key is { } key && Table?.Shown.FirstOrDefault(s => s.ModelIndex == cell.ColumnIndex)?.Column.Key is { } column ? key + ":" + column : string.Empty;

    /// <inheritdoc />
    protected override string GetLocalizedControlTypeCore() => DataTableStrings.Get("DataTable_ControlType_Cell");

    /// <inheritdoc />
    protected override bool IsControlElementCore() => cell.Role == CellRole.Data;

    /// <inheritdoc />
    protected override bool IsContentElementCore() => cell.Role == CellRole.Data;
}

/// <summary>
/// A row that is not made, told to assistive technology as a row that can be made on request: it has a
/// name and an id and one pattern, which makes the row. Asking for it changes nothing on screen.
/// </summary>
public sealed partial class DataTableRowStandInAutomationPeer : AutomationPeer, IVirtualizedItemProvider
{
    private readonly DataTable table;
    private readonly int index;
    private readonly string key;

    /// <summary>Makes the stand-in.</summary>
    /// <param name="owner">The table.</param>
    /// <param name="place">The row's place.</param>
    /// <param name="rowKey">The row's key.</param>
    public DataTableRowStandInAutomationPeer(DataTable owner, int place, string rowKey)
    {
        table = owner;
        index = place;
        key = rowKey;
    }

    /// <inheritdoc />
    public void Realize() => table.RealizeRow(index);

    /// <inheritdoc />
    protected override object? GetPatternCore(PatternInterface patternInterface) =>
        patternInterface == PatternInterface.VirtualizedItem ? this : null;

    /// <inheritdoc />
    protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.DataItem;

    /// <inheritdoc />
    protected override string GetClassNameCore() => nameof(DataTableRow);

    /// <inheritdoc />
    protected override string GetNameCore() =>
        table.Model is { } model && table.Presenter is { } presenter && index < model.RowCount ? presenter.RowName(model.GetRow(index)) : key;

    /// <inheritdoc />
    protected override string GetAutomationIdCore() => key;

    /// <inheritdoc />
    protected override bool IsOffscreenCore() => true;
}
