using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Navree.Design.Tables;

namespace Navree.Controls;

/// <summary>What a cell of a row is.</summary>
internal enum CellRole
{
    /// <summary>A column of the model.</summary>
    Data,

    /// <summary>The box that chooses the row.</summary>
    Check,

    /// <summary>The chevron that opens the row's detail.</summary>
    Chevron,
}

/// <summary>A cell of a body row: it holds what the host draws, and is the table's one tab stop when it is the active cell.</summary>
public sealed class DataTableCell : ContentControl
{
    /// <summary>Makes a cell.</summary>
    public DataTableCell()
    {
        DefaultStyleKey = typeof(DataTableCell);
    }

    /// <summary>What the cell is.</summary>
    internal CellRole Role { get; set; }

    /// <summary>The index of the cell's column among the model's columns; minus one for the gutter.</summary>
    internal int ColumnIndex { get; set; } = -1;

    /// <summary>Where the cell is among the cells of its row, counting the gutter: the column the grid's keys move by.</summary>
    internal int GridColumn { get; set; }

    /// <summary>The row the cell is in.</summary>
    internal DataTableRow? Row { get; set; }

    /// <summary>Whether the cell is the active one: only it is a tab stop.</summary>
    internal bool IsActive
    {
        get => IsTabStop;
        set => IsTabStop = value;
    }

    /// <inheritdoc />
    protected override Microsoft.UI.Xaml.Automation.Peers.AutomationPeer OnCreateAutomationPeer() => new DataTableCellAutomationPeer(this);

    /// <inheritdoc />
    protected override void OnGotFocus(RoutedEventArgs e)
    {
        base.OnGotFocus(e);
        Row?.Owner?.CellGotFocus(this, e);
    }

    /// <inheritdoc />
    protected override void OnPointerPressed(PointerRoutedEventArgs e)
    {
        base.OnPointerPressed(e);
        Row?.Owner?.CellPressed(this, e);
    }
}

/// <summary>A heading cell: the column's name, the mark of its sort, and the dots of its menu.</summary>
public sealed class DataTableHeaderCell : Control
{
    private TextBlock? text;
    private FontIcon? sortGlyph;
    private string heading = string.Empty;
    private DataTableAlign align;
    private DataTableSortDirection direction;
    private bool sortable;
    private bool over;
    private bool pressed;
    private bool menuAvailable;
    private bool resizable;
    private ResizeGrip? grip;
    private UIElement? options;

    /// <summary>Makes a heading cell.</summary>
    public DataTableHeaderCell()
    {
        DefaultStyleKey = typeof(DataTableHeaderCell);
    }

    /// <summary>The index of the cell's column among the model's columns; minus one for the gutter.</summary>
    internal int ColumnIndex { get; set; } = -1;

    /// <summary>Where the cell is among the cells of its row, counting the gutter.</summary>
    internal int GridColumn { get; set; }

    /// <summary>The row of headings the cell is in.</summary>
    internal DataTableRow? Row { get; set; }

    /// <summary>Whether the cell is the active one: only it is a tab stop.</summary>
    internal bool IsActive
    {
        get => IsTabStop;
        set => IsTabStop = value;
    }

    /// <summary>The words of the heading.</summary>
    internal string Heading => heading;

    /// <summary>The strip that widens the column, or null before the template is made.</summary>
    internal ResizeGrip? Grip => grip;

    /// <summary>The key of the column the heading is for.</summary>
    internal string ColumnKey => Row?.Owner?.Model is { } model && ColumnIndex >= 0 && ColumnIndex < model.Columns.Count ? model.Columns[ColumnIndex].Key : string.Empty;

    /// <summary>What the heading says of the sort, or null when it is not the sort.</summary>
    internal string? SortWords => direction switch
    {
        DataTableSortDirection.Ascending => DataTableStrings.Get("DataTable_Status_Ascending"),
        DataTableSortDirection.Descending => DataTableStrings.Get("DataTable_Status_Descending"),
        _ => null,
    };

    /// <summary>Whether a press on the heading asks for a sort.</summary>
    internal bool Sortable => sortable;

    /// <summary>Sets what the cell shows.</summary>
    /// <param name="words">The heading.</param>
    /// <param name="alignment">Where the words sit.</param>
    /// <param name="sort">How the column is sorted now, or null when it cannot be.</param>
    /// <param name="hasMenu">Whether the cell shows the dots of a menu when pointed at.</param>
    /// <param name="canResize">Whether a pointer can widen the column.</param>
    internal void Set(string words, DataTableAlign alignment, DataTableSort? sort, bool hasMenu, bool canResize)
    {
        heading = words;
        align = alignment;
        sortable = sort is not null;
        direction = sort?.Direction ?? DataTableSortDirection.None;
        menuAvailable = hasMenu;
        resizable = canResize;
        Refresh();
    }

    /// <inheritdoc />
    protected override void OnApplyTemplate()
    {
        base.OnApplyTemplate();
        text = GetTemplateChild("PART_Text") as TextBlock;
        sortGlyph = GetTemplateChild("PART_SortGlyph") as FontIcon;
        options = GetTemplateChild("PART_Options") as UIElement;
        if (GetTemplateChild("PART_Grip") is ResizeGrip made && !ReferenceEquals(made, grip))
        {
            grip = made;
            made.Started += () => Row?.Owner?.ResizeStarted(this);
            made.Moved += dx => Row?.Owner?.ResizeMoved(this, dx);
            made.Ended += keep => Row?.Owner?.ResizeEnded(this, keep);
            made.FitRequested += () => Row?.Owner?.FitColumn(this);
        }

        Refresh();
    }

    /// <inheritdoc />
    protected override void OnPointerEntered(PointerRoutedEventArgs e)
    {
        base.OnPointerEntered(e);
        over = e.Pointer.PointerDeviceType != Microsoft.UI.Input.PointerDeviceType.Touch;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerExited(PointerRoutedEventArgs e)
    {
        base.OnPointerExited(e);
        over = false;
        pressed = false;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnPointerPressed(PointerRoutedEventArgs e)
    {
        base.OnPointerPressed(e);
        pressed = true;
        UpdateState();
        Row?.Owner?.HeaderPressed(this, e);
    }

    /// <inheritdoc />
    protected override void OnPointerReleased(PointerRoutedEventArgs e)
    {
        base.OnPointerReleased(e);
        pressed = false;
        UpdateState();
    }

    /// <inheritdoc />
    protected override void OnTapped(TappedRoutedEventArgs e)
    {
        base.OnTapped(e);
        if (sortable)
        {
            Row?.Owner?.SortPressed(this);
            e.Handled = true;
        }
    }

    /// <inheritdoc />
    protected override Microsoft.UI.Xaml.Automation.Peers.AutomationPeer OnCreateAutomationPeer() => new DataTableHeaderItemAutomationPeer(this);

    /// <inheritdoc />
    protected override void OnRightTapped(RightTappedRoutedEventArgs e)
    {
        base.OnRightTapped(e);
        if (!e.Handled && Row?.Owner is { } owner)
        {
            owner.ShowColumnMenu(this, e.GetPosition(owner));
            e.Handled = true;
        }
    }

    /// <inheritdoc />
    protected override void OnGotFocus(RoutedEventArgs e)
    {
        base.OnGotFocus(e);
        Row?.Owner?.HeaderGotFocus(this, e);
    }

    private void UpdateState() =>
        VisualStateManager.GoToState(this, pressed ? "Pressed" : over ? "PointerOver" : "Normal", false);

    private void Refresh()
    {
        if (text is null)
        {
            return;
        }

        text.Text = heading;
        text.HorizontalAlignment = align switch
        {
            DataTableAlign.End => HorizontalAlignment.Right,
            DataTableAlign.Center => HorizontalAlignment.Center,
            _ => HorizontalAlignment.Left,
        };
        if (sortGlyph is not null)
        {
            // Up and down chevrons of the symbol font; they do not mirror.
            sortGlyph.Glyph = direction == DataTableSortDirection.Descending ? Glyphs.SortDescending : Glyphs.SortAscending;
            sortGlyph.Visibility = direction == DataTableSortDirection.None ? Visibility.Collapsed : Visibility.Visible;
        }

        if (grip is not null)
        {
            grip.Visibility = resizable ? Visibility.Visible : Visibility.Collapsed;
        }

        if (options is not null)
        {
            options.Visibility = menuAvailable ? Visibility.Visible : Visibility.Collapsed;
        }

        UpdateState();
    }
}
