using System.Globalization;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Navree.Design.Tables;
using Windows.Foundation;

namespace Navree.Controls;

/// <summary>
/// A table of rows that arrive by cursor, with columns the server names. It draws what its model
/// holds, asks its host for more rows near the ends, and tells its host what a person did; it knows
/// nothing of any protocol. Rows are of one height, so a row's place is arithmetic and a hundred
/// thousand rows cost no more than twenty. The header and the frozen columns stay in place while the
/// rest scrolls, moved on the compositor.
/// </summary>
public sealed partial class DataTable : Control
{
    /// <summary>The rows and columns.</summary>
    public static readonly DependencyProperty ModelProperty = DependencyProperty.Register(
        nameof(Model),
        typeof(IDataTableModel),
        typeof(DataTable),
        new PropertyMetadata(null, (d, e) => ((DataTable)d).ModelChanged((IDataTableModel?)e.OldValue, (IDataTableModel?)e.NewValue)));

    /// <summary>What draws the cells.</summary>
    public static readonly DependencyProperty PresenterProperty = DependencyProperty.Register(
        nameof(Presenter),
        typeof(IDataTablePresenter),
        typeof(DataTable),
        new PropertyMetadata(null, (d, e) => ((DataTable)d).PresenterChanged()));

    /// <summary>What stands above the header: a search box, filters.</summary>
    public static readonly DependencyProperty ToolbarProperty = DependencyProperty.Register(
        nameof(Toolbar),
        typeof(UIElement),
        typeof(DataTable),
        new PropertyMetadata(null));

    /// <summary>What is shown when there are no rows.</summary>
    public static readonly DependencyProperty EmptyContentProperty = DependencyProperty.Register(
        nameof(EmptyContent),
        typeof(UIElement),
        typeof(DataTable),
        new PropertyMetadata(null, (d, e) => ((DataTable)d).UpdateState()));

    /// <summary>The table's name for assistive technology.</summary>
    public static readonly DependencyProperty CaptionProperty = DependencyProperty.Register(
        nameof(Caption),
        typeof(string),
        typeof(DataTable),
        new PropertyMetadata(null));

    private Density density = Density.Standard;
    private CultureInfo culture = CultureInfo.CurrentCulture;
    private string? layoutKey;
    private IDataTableLayoutStore? layoutStore;

    /// <summary>Makes a table.</summary>
    public DataTable()
    {
        DefaultStyleKey = typeof(DataTable);
        Loaded += (_, _) => OnLoaded();
        Unloaded += (_, _) => OnUnloaded();
        SizeChanged += (_, _) => OnSizeChanged();
        PreviewKeyDown += OnPreviewKeyDownCore;
    }

    /// <summary>A person asked for a column to be sorted.</summary>
    public event TypedEventHandler<DataTable, DataTableSortRequestedEventArgs>? SortRequested;

    /// <summary>A row was invoked: its action is to run.</summary>
    public event TypedEventHandler<DataTable, DataTableRowEventArgs>? RowInvoked;

    /// <summary>More rows are wanted at an end.</summary>
    public event TypedEventHandler<DataTable, DataTableMoreRequestedEventArgs>? MoreRequested;

    /// <summary>Rows were chosen or cleared by a person.</summary>
    public event TypedEventHandler<DataTable, DataTableChoiceRequestedEventArgs>? ChoiceRequested;

    /// <summary>A row's detail was opened or shut by a person.</summary>
    public event TypedEventHandler<DataTable, DataTableDetailEventArgs>? DetailToggled;

    /// <summary>A person asked to edit a row.</summary>
    public event TypedEventHandler<DataTable, DataTableRowEventArgs>? EditRequested;

    /// <summary>A person pressed try again.</summary>
    public event TypedEventHandler<DataTable, DataTableRetryEventArgs>? RetryRequested;

    /// <summary>A person asked for the rows to be fetched again.</summary>
    public event TypedEventHandler<DataTable, object?>? RefreshRequested;

    /// <summary>The most rows the table holds were reached.</summary>
    public event TypedEventHandler<DataTable, object?>? HeldLimitReached;

    /// <summary>The table became a table or a list.</summary>
    public event TypedEventHandler<DataTable, DataTableModeEventArgs>? ModeChanged;

    /// <summary>A person changed the columns: widths, order, what is hidden or frozen, the density.</summary>
    public event TypedEventHandler<DataTable, object?>? LayoutChanged;

    /// <summary>The rows and columns.</summary>
    public IDataTableModel? Model
    {
        get => (IDataTableModel?)GetValue(ModelProperty);
        set => SetValue(ModelProperty, value);
    }

    /// <summary>What draws the cells.</summary>
    public IDataTablePresenter? Presenter
    {
        get => (IDataTablePresenter?)GetValue(PresenterProperty);
        set => SetValue(PresenterProperty, value);
    }

    /// <summary>What stands above the header; it gives way to the bar of bulk actions while any row is chosen.</summary>
    public UIElement? Toolbar
    {
        get => (UIElement?)GetValue(ToolbarProperty);
        set => SetValue(ToolbarProperty, value);
    }

    /// <summary>What is shown when there are no rows; null for the table's own words.</summary>
    public UIElement? EmptyContent
    {
        get => (UIElement?)GetValue(EmptyContentProperty);
        set => SetValue(EmptyContentProperty, value);
    }

    /// <summary>The table's name for assistive technology.</summary>
    public string? Caption
    {
        get => (string?)GetValue(CaptionProperty);
        set => SetValue(CaptionProperty, value);
    }

    /// <summary>The commands that act on the chosen rows.</summary>
    public IList<ICommandBarElement> BulkActions { get; } = new List<ICommandBarElement>();

    /// <summary>How tall the rows are; what a person chose is kept and wins once a layout key is set.</summary>
    public Density Density
    {
        get => density;
        set
        {
            if (density != value)
            {
                density = value;
                RowMetricsChanged();
            }
        }
    }

    /// <summary>The culture the counts and the words around them are written in.</summary>
    public CultureInfo Culture
    {
        get => culture;
        set => culture = value ?? CultureInfo.CurrentCulture;
    }

    /// <summary>What a person's layout is kept under; null keeps nothing.</summary>
    public string? LayoutKey
    {
        get => layoutKey;
        set => layoutKey = value;
    }

    /// <summary>Where a person's layout is kept.</summary>
    public IDataTableLayoutStore? LayoutStore
    {
        get => layoutStore;
        set => layoutStore = value;
    }

    /// <summary>Whether the table is drawn as a table or as a list.</summary>
    public DataTableMode Mode { get; private set; } = DataTableMode.Table;

    /// <summary>How many rows are chosen, counting those held.</summary>
    public int ChosenCount => choice.Across ? ChoosableHeld() : choice.Keys.Count;

    /// <summary>Whether every record that matches is chosen, held or not.</summary>
    public bool AcrossChosen => choice.Across;

    /// <summary>The active cell, by the keys of its row and its column; null when none.</summary>
    public DataTableCellRef? ActiveCell
    {
        get
        {
            if (Model is not { } model || activeRow < 0 || activeRow >= model.RowCount || model.GetRow(activeRow).Key is not { } rowKey)
            {
                return null;
            }

            return ActiveColumnKey() is { } columnKey ? new DataTableCellRef(rowKey, columnKey) : null;
        }
    }

    /// <summary>Replaces the chosen set, as the host's answer says.</summary>
    /// <param name="keys">The keys of the rows chosen.</param>
    /// <param name="across">Whether every record that matches is chosen.</param>
    public void ApplyChoice(IEnumerable<string> keys, bool across)
    {
        choice = new ChoiceState(new HashSet<string>(keys, StringComparer.Ordinal), across);
        ChoiceChanged(announce: false);
    }

    /// <summary>Gives focus to a row, and to one of its cells.</summary>
    /// <param name="key">The row's key.</param>
    /// <param name="columnKey">The column's key; the active column when null.</param>
    /// <returns>False when the row is not held.</returns>
    public bool FocusRow(string key, string? columnKey = null)
    {
        if (Model is not { } model)
        {
            return false;
        }

        int row = model.IndexOfKey(key);
        if (row < 0)
        {
            return false;
        }

        int column = columnKey is null ? activeColumn : GridColumnOf(columnKey);
        SetActive(new Cell(row, Math.Max(column, 0)), focus: true);
        return true;
    }

    /// <summary>Scrolls a row into view.</summary>
    /// <param name="key">The row's key.</param>
    public void ScrollToRow(string key)
    {
        if (Model is { } model && model.IndexOfKey(key) is var row and >= 0)
        {
            RevealRow(row);
        }
    }

    /// <summary>Puts focus back on the cell that had it, after a task, a dialog or a menu the host opened.</summary>
    public void RestoreFocus() => FocusActive();

    /// <summary>Forgets what a person kept of the columns.</summary>
    public void ResetLayout() => ForgetLayout();

    private void RaiseSortRequested(DataTableColumn column) => SortRequested?.Invoke(this, new DataTableSortRequestedEventArgs(column));

    private void RaiseRowInvoked(string key) => RowInvoked?.Invoke(this, new DataTableRowEventArgs(key));

    private void RaiseMoreRequested(MoreDirection direction) => MoreRequested?.Invoke(this, new DataTableMoreRequestedEventArgs(direction));

    private void RaiseChoiceRequested(IReadOnlyList<string> keys, bool on, bool all, bool across) =>
        ChoiceRequested?.Invoke(this, new DataTableChoiceRequestedEventArgs(keys, on, all, across));

    private void RaiseDetailToggled(string key, bool open) => DetailToggled?.Invoke(this, new DataTableDetailEventArgs(key, open));

    private void RaiseEditRequested(string key) => EditRequested?.Invoke(this, new DataTableRowEventArgs(key));

    private void RaiseRetryRequested(RetryKind kind) => RetryRequested?.Invoke(this, new DataTableRetryEventArgs(kind));

    private void RaiseRefreshRequested() => RefreshRequested?.Invoke(this, null);

    private void RaiseHeldLimitReached() => HeldLimitReached?.Invoke(this, null);

    private void RaiseLayoutChanged() => LayoutChanged?.Invoke(this, null);

    private void RaiseModeChanged() => ModeChanged?.Invoke(this, new DataTableModeEventArgs(Mode));
}
