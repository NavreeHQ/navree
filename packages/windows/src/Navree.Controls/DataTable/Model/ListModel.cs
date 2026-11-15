using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Navree.Design.Tables;

namespace Navree.Controls;

/// <summary>A column of a table of items of one type, with the words each cell shows.</summary>
/// <typeparam name="T">The type of an item.</typeparam>
/// <param name="Key">The column's identity.</param>
/// <param name="Header">The heading.</param>
/// <param name="Text">The words of the column's cell for an item.</param>
/// <param name="Kind">What the column holds.</param>
/// <param name="Align">Where its words sit.</param>
/// <param name="Sortable">Whether a heading press asks for a sort.</param>
/// <param name="Role">What it is to its row, for the list form.</param>
/// <param name="Secondary">Whether a narrow window may leave it out.</param>
public sealed record DataTableColumn<T>(
    string Key,
    string Header,
    Func<T, string> Text,
    ColumnKind Kind = ColumnKind.Words,
    DataTableAlign Align = DataTableAlign.Start,
    bool Sortable = false,
    ColumnRole Role = ColumnRole.None,
    bool Secondary = false);

/// <summary>A model over a list of items, for anybody who has no protocol: it holds the items and tells the control of changes.</summary>
/// <typeparam name="T">The type of an item.</typeparam>
public sealed class ListDataTableModel<T> : IDataTableModel
{
    private readonly List<T> items = [];
    private readonly Dictionary<string, int> places = new(StringComparer.Ordinal);
    private readonly Func<T, string> keyOf;
    private List<DataTableColumn> columns = [];
    private DataTableLoadState state;
    private string? failure;
    private DataTableCursor later;
    private DataTableCursor earlier;
    private string? summary;
    private DataTableChoiceOptions choice;

    /// <summary>Makes a model.</summary>
    /// <param name="columns">The columns.</param>
    /// <param name="key">The key of an item.</param>
    public ListDataTableModel(IEnumerable<DataTableColumn<T>> columns, Func<T, string> key)
    {
        keyOf = key;
        Source = columns.ToList();
        this.columns = Source.Select(ToColumn).ToList();
    }

    /// <inheritdoc />
    public event EventHandler<DataTableModelChangedEventArgs>? Changed;

    /// <summary>The columns as the host described them, with the words of each cell.</summary>
    public IReadOnlyList<DataTableColumn<T>> Source { get; }

    /// <summary>Whether an item can be chosen; every item can by default.</summary>
    public Func<T, Choosable> ChoosableOf { get; set; } = _ => Choosable.Yes;

    /// <summary>Whether a click on an item does something; none does by default.</summary>
    public Func<T, bool> HasActionOf { get; set; } = _ => false;

    /// <summary>Whether an item has a menu.</summary>
    public Func<T, bool> HasMenuOf { get; set; } = _ => false;

    /// <summary>Whether an item has a detail.</summary>
    public Func<T, bool> HasDetailOf { get; set; } = _ => false;

    /// <summary>Whether an item can be edited.</summary>
    public Func<T, bool> HasEditOf { get; set; } = _ => false;

    /// <inheritdoc />
    public IReadOnlyList<DataTableColumn> Columns => columns;

    /// <inheritdoc />
    public int RowCount => items.Count;

    /// <inheritdoc />
    public DataTableLoadState State
    {
        get => state;
        set
        {
            state = value;
            Raise(DataTableChangeKind.State);
        }
    }

    /// <inheritdoc />
    public string? FailureMessage
    {
        get => failure;
        set
        {
            failure = value;
            Raise(DataTableChangeKind.State);
        }
    }

    /// <inheritdoc />
    public DataTableCursor Later
    {
        get => later;
        set
        {
            later = value;
            Raise(DataTableChangeKind.Cursor);
        }
    }

    /// <inheritdoc />
    public DataTableCursor Earlier
    {
        get => earlier;
        set
        {
            earlier = value;
            Raise(DataTableChangeKind.Cursor);
        }
    }

    /// <inheritdoc />
    public string? Summary
    {
        get => summary;
        set
        {
            summary = value;
            Raise(DataTableChangeKind.Summary);
        }
    }

    /// <inheritdoc />
    public DataTableChoiceOptions Choice
    {
        get => choice;
        set
        {
            choice = value;
            Raise(DataTableChangeKind.State);
        }
    }

    /// <summary>The item at a place.</summary>
    /// <param name="index">The place.</param>
    /// <returns>The item.</returns>
    public T ItemAt(int index) => items[index];

    /// <summary>Replaces every item.</summary>
    /// <param name="rows">The new items.</param>
    public void Reset(IEnumerable<T> rows)
    {
        items.Clear();
        items.AddRange(rows);
        Reindex();
        Raise(DataTableChangeKind.Reset);
    }

    /// <summary>Adds items at the end.</summary>
    /// <param name="rows">The items.</param>
    public void Append(IEnumerable<T> rows)
    {
        int at = items.Count;
        items.AddRange(rows);
        for (int i = at; i < items.Count; i++)
        {
            places[keyOf(items[i])] = i;
        }

        Raise(DataTableChangeKind.Insert, at, items.Count - at);
    }

    /// <summary>Adds items at the start.</summary>
    /// <param name="rows">The items.</param>
    public void Prepend(IEnumerable<T> rows)
    {
        List<T> added = rows.ToList();
        items.InsertRange(0, added);
        Reindex();
        Raise(DataTableChangeKind.Insert, 0, added.Count);
    }

    /// <summary>Takes an item out.</summary>
    /// <param name="key">The item's key.</param>
    public void Remove(string key)
    {
        if (places.TryGetValue(key, out int at))
        {
            items.RemoveAt(at);
            Reindex();
            Raise(DataTableChangeKind.Remove, at, 1);
        }
    }

    /// <summary>Draws an item again with new data; its key stays.</summary>
    /// <param name="row">The item.</param>
    public void Replace(T row)
    {
        if (places.TryGetValue(keyOf(row), out int at))
        {
            items[at] = row;
            Raise(DataTableChangeKind.Replace, at, 1);
        }
    }

    /// <inheritdoc />
    public DataTableRecord GetRow(int index)
    {
        T item = items[index];
        return new DataTableRecord(
            keyOf(item),
            ChoosableOf(item),
            HasActionOf(item),
            HasMenuOf(item),
            HasDetailOf(item),
            false,
            HasEditOf(item),
            item);
    }

    /// <inheritdoc />
    public int IndexOfKey(string key) => places.TryGetValue(key, out int at) ? at : -1;

    private static DataTableColumn ToColumn(DataTableColumn<T> column) => new(
        column.Key,
        column.Header,
        column.Kind,
        column.Role,
        column.Align,
        column.Sortable ? new DataTableSort(DataTableSortDirection.None, column.Key) : null,
        column.Secondary);

    /// <summary>Sets how a column is sorted, as the server's answer says.</summary>
    /// <param name="key">The column's key.</param>
    /// <param name="direction">The way it is sorted now.</param>
    public void SetSort(string? key, DataTableSortDirection direction)
    {
        columns = columns
            .Select(column => column.Sort is { } sort
                ? column with { Sort = sort with { Direction = column.Key == key ? direction : DataTableSortDirection.None } }
                : column)
            .ToList();
        Raise(DataTableChangeKind.Columns);
    }

    private void Reindex()
    {
        places.Clear();
        for (int i = 0; i < items.Count; i++)
        {
            places[keyOf(items[i])] = i;
        }
    }

    private void Raise(DataTableChangeKind kind, int index = 0, int count = 0) =>
        Changed?.Invoke(this, new DataTableModelChangedEventArgs(kind, index, count));
}

/// <summary>Draws a table of items as words: each cell is a line of text.</summary>
/// <typeparam name="T">The type of an item.</typeparam>
public sealed class TextTablePresenter<T> : IDataTablePresenter
{
    private readonly ListDataTableModel<T> model;

    /// <summary>Makes a presenter for a model.</summary>
    /// <param name="model">The model.</param>
    public TextTablePresenter(ListDataTableModel<T> model)
    {
        this.model = model;
    }

    /// <summary>Makes the content of a detail, or null for none.</summary>
    public Func<T, UIElement?>? Detail { get; set; }

    /// <summary>Makes the menu of an item, or null for none.</summary>
    public Func<T, FlyoutBase?>? Menu { get; set; }

    /// <summary>The line under an item's name, or null for none.</summary>
    public Func<T, string?>? Second { get; set; }

    /// <inheritdoc />
    public UIElement CreateCell(DataTableColumn column, RowShape shape) => column.Role == ColumnRole.Actions
        ? new DataTableGlyph { Glyph = Glyphs.More, Width = 32, Height = 32, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center }
        : Words(column);

    // A cell of words: when its column is too narrow the words are cut with an ellipsis, and all of them are in a tooltip.
    private static TextBlock Words(DataTableColumn column)
    {
        TextBlock block = new()
        {
            TextTrimming = column.Kind == ColumnKind.Number ? TextTrimming.Clip : TextTrimming.CharacterEllipsis,
            TextWrapping = TextWrapping.NoWrap,
            VerticalAlignment = VerticalAlignment.Center,
            HorizontalAlignment = column.Align switch
            {
                DataTableAlign.End => HorizontalAlignment.Right,
                DataTableAlign.Center => HorizontalAlignment.Center,
                _ => HorizontalAlignment.Left,
            },
        };
        block.IsTextTrimmedChanged += (sender, _) =>
            ToolTipService.SetToolTip(sender, sender.IsTextTrimmed ? sender.Text : null);
        return block;
    }

    /// <inheritdoc />
    public void BindCell(UIElement cell, in DataTableRecord record, int columnIndex)
    {
        if (cell is TextBlock text)
        {
            text.Text = CellText(record, columnIndex);
        }
        else if (cell is DataTableGlyph glyph)
        {
            glyph.Visibility = record.HasMenu ? Visibility.Visible : Visibility.Collapsed;
        }
    }

    /// <inheritdoc />
    public void ClearCell(UIElement cell)
    {
        if (cell is TextBlock text)
        {
            text.Text = string.Empty;
        }
    }

    /// <inheritdoc />
    public string CellText(in DataTableRecord record, int columnIndex) =>
        record.Data is T item && model.Source[columnIndex].Role != ColumnRole.Actions ? model.Source[columnIndex].Text(item) : string.Empty;

    /// <inheritdoc />
    public string? SecondLine(in DataTableRecord record) => record.Data is T item ? Second?.Invoke(item) : null;

    /// <inheritdoc />
    public string RowName(in DataTableRecord record) => model.Source.Count > 0 ? CellText(record, 0) : record.Key ?? string.Empty;

    /// <inheritdoc />
    public int ControlCount(in DataTableRecord record, int columnIndex) =>
        model.Source[columnIndex].Role == ColumnRole.Actions && record.HasMenu ? 1 : 0;

    /// <inheritdoc />
    public UIElement? CreateDetail(in DataTableRecord record) => record.Data is T item ? Detail?.Invoke(item) : null;

    /// <inheritdoc />
    public FlyoutBase? CreateRowMenu(in DataTableRecord record) => record.Data is T item ? Menu?.Invoke(item) : null;
}
