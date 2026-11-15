using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls.Primitives;
using Navree.Design.Tables;

namespace Navree.Controls;

/// <summary>Where a column's words sit.</summary>
public enum DataTableAlign
{
    /// <summary>At the inline start.</summary>
    Start,

    /// <summary>In the middle.</summary>
    Center,

    /// <summary>At the inline end.</summary>
    End,
}

/// <summary>The way a column is sorted.</summary>
public enum DataTableSortDirection
{
    /// <summary>It is not the sort.</summary>
    None,

    /// <summary>Smallest first.</summary>
    Ascending,

    /// <summary>Largest first.</summary>
    Descending,
}

/// <summary>The state of the table as a whole while it holds no rows.</summary>
public enum DataTableLoadState
{
    /// <summary>It has what it was asked for.</summary>
    Ready,

    /// <summary>The first rows are on their way.</summary>
    Loading,

    /// <summary>The first rows could not be had.</summary>
    Failed,
}

/// <summary>The state of one end of the rows.</summary>
public enum CursorState
{
    /// <summary>Nothing is on its way.</summary>
    Idle,

    /// <summary>Rows are on their way.</summary>
    Loading,

    /// <summary>The rows could not be had.</summary>
    Failed,
}

/// <summary>Whether the table is drawn as a table or as a list.</summary>
public enum DataTableMode
{
    /// <summary>Columns with headings.</summary>
    Table,

    /// <summary>Each record folded into a row of a list.</summary>
    List,
}

/// <summary>Which end of the rows more are asked for.</summary>
public enum MoreDirection
{
    /// <summary>The rows after the last held.</summary>
    After,

    /// <summary>The rows before the first held.</summary>
    Before,
}

/// <summary>What a try again is for.</summary>
public enum RetryKind
{
    /// <summary>The first rows.</summary>
    Initial,

    /// <summary>The rows after the last.</summary>
    More,

    /// <summary>The rows before the first.</summary>
    Earlier,
}

/// <summary>How a column is sorted, and where a heading asks for it to be.</summary>
/// <param name="Direction">The way the table is sorted by this column now.</param>
/// <param name="Address">What the host needs to ask for the sort; the table never reads it.</param>
public readonly record struct DataTableSort(DataTableSortDirection Direction, string Address);

/// <summary>A column of a table.</summary>
/// <param name="Key">The column's identity across answers.</param>
/// <param name="Header">The heading.</param>
/// <param name="Kind">What the column holds, which sets how wide it asks to be.</param>
/// <param name="Role">What the column is to its row, for the list form.</param>
/// <param name="Align">Where its words sit.</param>
/// <param name="Sort">How it sorts, or null.</param>
/// <param name="Secondary">Whether a narrow window may leave it out.</param>
public readonly record struct DataTableColumn(
    string Key,
    string Header,
    ColumnKind Kind,
    ColumnRole Role,
    DataTableAlign Align,
    DataTableSort? Sort,
    bool Secondary);

/// <summary>A record of a table, as the table reads it. The record's own data is the host's.</summary>
/// <param name="Key">The record's key: what a chosen row is sent by. Null for a row with no key.</param>
/// <param name="Choosable">Whether the row can be chosen.</param>
/// <param name="HasAction">Whether a click on the row does something.</param>
/// <param name="HasMenu">Whether the row has a menu.</param>
/// <param name="HasDetail">Whether the row has a detail.</param>
/// <param name="DetailOpen">Whether the detail is open when the row first arrives.</param>
/// <param name="HasEdit">Whether the row can be edited.</param>
/// <param name="Data">The host's own data for the row.</param>
public readonly record struct DataTableRecord(
    string? Key,
    Choosable Choosable,
    bool HasAction,
    bool HasMenu,
    bool HasDetail,
    bool DetailOpen,
    bool HasEdit,
    object? Data);

/// <summary>Whether there are more rows at an end, and what is happening to them.</summary>
/// <param name="Available">Whether more can be asked for.</param>
/// <param name="State">What is happening to the rows asked for.</param>
public readonly record struct DataTableCursor(bool Available, CursorState State);

/// <summary>How the rows may be chosen.</summary>
/// <param name="Available">Whether rows can be chosen at all.</param>
/// <param name="AcrossAvailable">Whether every record that matches may be chosen, held or not.</param>
/// <param name="AcrossTotal">How many records match, when the server counts them.</param>
public readonly record struct DataTableChoiceOptions(bool Available, bool AcrossAvailable, long? AcrossTotal);

/// <summary>A cell by the keys of its row and its column.</summary>
/// <param name="RowKey">The row's key.</param>
/// <param name="ColumnKey">The column's key.</param>
public readonly record struct DataTableCellRef(string RowKey, string ColumnKey);

/// <summary>What changed in a model.</summary>
public enum DataTableChangeKind
{
    /// <summary>Every row is new.</summary>
    Reset,

    /// <summary>Rows were added.</summary>
    Insert,

    /// <summary>Rows were taken out.</summary>
    Remove,

    /// <summary>A row was drawn again.</summary>
    Replace,

    /// <summary>The columns changed.</summary>
    Columns,

    /// <summary>The state of the table as a whole changed.</summary>
    State,

    /// <summary>The state of an end of the rows changed.</summary>
    Cursor,

    /// <summary>The summary changed.</summary>
    Summary,
}

/// <summary>What changed in a model, and where.</summary>
public sealed class DataTableModelChangedEventArgs : EventArgs
{
    /// <summary>Makes the arguments.</summary>
    /// <param name="kind">What changed.</param>
    /// <param name="index">Where the first changed row is.</param>
    /// <param name="count">How many rows.</param>
    public DataTableModelChangedEventArgs(DataTableChangeKind kind, int index = 0, int count = 0)
    {
        Kind = kind;
        Index = index;
        Count = count;
    }

    /// <summary>What changed.</summary>
    public DataTableChangeKind Kind { get; }

    /// <summary>Where the first changed row is.</summary>
    public int Index { get; }

    /// <summary>How many rows.</summary>
    public int Count { get; }
}

/// <summary>The rows and columns of a table, as the control reads them.</summary>
public interface IDataTableModel
{
    /// <summary>The columns.</summary>
    IReadOnlyList<DataTableColumn> Columns { get; }

    /// <summary>How many rows are held.</summary>
    int RowCount { get; }

    /// <summary>The state of the table as a whole while it holds no rows.</summary>
    DataTableLoadState State { get; }

    /// <summary>The words that say why the first rows could not be had.</summary>
    string? FailureMessage { get; }

    /// <summary>The rows after the last held.</summary>
    DataTableCursor Later { get; }

    /// <summary>The rows before the first held.</summary>
    DataTableCursor Earlier { get; }

    /// <summary>How many there are, in the server's words.</summary>
    string? Summary { get; }

    /// <summary>How the rows may be chosen.</summary>
    DataTableChoiceOptions Choice { get; }

    /// <summary>Tells the control something changed.</summary>
    event EventHandler<DataTableModelChangedEventArgs>? Changed;

    /// <summary>A row, by its place. Cheap and immutable.</summary>
    /// <param name="index">The place.</param>
    /// <returns>The row.</returns>
    DataTableRecord GetRow(int index);

    /// <summary>The place of a row by its key.</summary>
    /// <param name="key">The key.</param>
    /// <returns>The place, or minus one when the row is not held.</returns>
    int IndexOfKey(string key);
}

/// <summary>What the host supplies so the control can draw a model's cells.</summary>
public interface IDataTablePresenter
{
    /// <summary>Makes the content of a cell; the row keeps it and binds it to one record after another.</summary>
    /// <param name="column">The column the cell is in.</param>
    /// <param name="shape">Whether the rows have one line or two.</param>
    /// <returns>The content.</returns>
    UIElement CreateCell(DataTableColumn column, RowShape shape);

    /// <summary>Binds a cell's content to a record.</summary>
    /// <param name="cell">The content made by <see cref="CreateCell"/>.</param>
    /// <param name="record">The record.</param>
    /// <param name="columnIndex">The index of the column among the model's columns.</param>
    void BindCell(UIElement cell, in DataTableRecord record, int columnIndex);

    /// <summary>Lets go of what a cell was bound to.</summary>
    /// <param name="cell">The content.</param>
    void ClearCell(UIElement cell);

    /// <summary>The words of a cell: for sizing, tooltips, accessibility, copy and type-ahead.</summary>
    /// <param name="record">The record.</param>
    /// <param name="columnIndex">The index of the column among the model's columns.</param>
    /// <returns>The words.</returns>
    string CellText(in DataTableRecord record, int columnIndex);

    /// <summary>The line under the name, or null. When any of the first rows has one, every row of the table is two lines tall.</summary>
    /// <param name="record">The record.</param>
    /// <returns>The words, or null.</returns>
    string? SecondLine(in DataTableRecord record);

    /// <summary>The name of the record, for type-ahead and accessibility.</summary>
    /// <param name="record">The record.</param>
    /// <returns>The name.</returns>
    string RowName(in DataTableRecord record);

    /// <summary>How many controls a cell holds.</summary>
    /// <param name="record">The record.</param>
    /// <param name="columnIndex">The index of the column among the model's columns.</param>
    /// <returns>The count.</returns>
    int ControlCount(in DataTableRecord record, int columnIndex);

    /// <summary>Makes what stands under an open row.</summary>
    /// <param name="record">The record.</param>
    /// <returns>The content, or null.</returns>
    UIElement? CreateDetail(in DataTableRecord record);

    /// <summary>Makes the menu of a row.</summary>
    /// <param name="record">The record.</param>
    /// <returns>The menu, or null for none.</returns>
    FlyoutBase? CreateRowMenu(in DataTableRecord record);
}

/// <summary>Keeps what a person did to a table's columns.</summary>
public interface IDataTableLayoutStore
{
    /// <summary>Reads a kept text.</summary>
    /// <param name="key">What it is kept under.</param>
    /// <param name="json">The text.</param>
    /// <returns>True when there is one.</returns>
    bool TryLoad(string key, out string json);

    /// <summary>Keeps a text.</summary>
    /// <param name="key">What to keep it under.</param>
    /// <param name="json">The text.</param>
    void Save(string key, string json);
}

/// <summary>A sort was asked for.</summary>
public sealed class DataTableSortRequestedEventArgs(DataTableColumn column) : EventArgs
{
    /// <summary>The column whose heading was pressed.</summary>
    public DataTableColumn Column { get; } = column;
}

/// <summary>Something was done to a row.</summary>
public sealed class DataTableRowEventArgs(string key) : EventArgs
{
    /// <summary>The row's key.</summary>
    public string Key { get; } = key;
}

/// <summary>More rows were asked for.</summary>
public sealed class DataTableMoreRequestedEventArgs(MoreDirection direction) : EventArgs
{
    /// <summary>Which end.</summary>
    public MoreDirection Direction { get; } = direction;
}

/// <summary>Rows were chosen or cleared.</summary>
public sealed class DataTableChoiceRequestedEventArgs(IReadOnlyList<string> keys, bool on, bool all, bool across) : EventArgs
{
    /// <summary>The keys of the rows.</summary>
    public IReadOnlyList<string> Keys { get; } = keys;

    /// <summary>Whether they are chosen (true) or cleared (false).</summary>
    public bool On { get; } = on;

    /// <summary>Whether it is every held row that can be chosen, and no key is given.</summary>
    public bool All { get; } = all;

    /// <summary>Whether it is every record that matches.</summary>
    public bool Across { get; } = across;
}

/// <summary>A row's detail was opened or shut.</summary>
public sealed class DataTableDetailEventArgs(string key, bool open) : EventArgs
{
    /// <summary>The row's key.</summary>
    public string Key { get; } = key;

    /// <summary>Whether the detail is open now.</summary>
    public bool Open { get; } = open;
}

/// <summary>A try again was pressed.</summary>
public sealed class DataTableRetryEventArgs(RetryKind kind) : EventArgs
{
    /// <summary>What to try again.</summary>
    public RetryKind Kind { get; } = kind;
}

/// <summary>The table became a table or a list.</summary>
public sealed class DataTableModeEventArgs(DataTableMode mode) : EventArgs
{
    /// <summary>The mode now.</summary>
    public DataTableMode Mode { get; } = mode;
}
